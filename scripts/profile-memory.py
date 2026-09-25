#!/usr/bin/env python3
"""Linux-only, synthetic PTY memory experiments. No real Pi or clipboard access.

Build first: cargo build --release --locked
Run: python3 scripts/profile-memory.py --repeats 3 --output /tmp/hibiscus-memory.json
Numbers are process RSS/high-water observations, not heap attribution or CI gates.
"""
import argparse
import errno
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shlex
import signal
import statistics
import struct
import subprocess
import sys
import tempfile
import termios
import time

MIB = 1024 * 1024
SCENARIOS = {
    "idle": {},
    "long-chat": {"turns": 20},
    "image-send": {"images": 4, "image_mib": 2},
    "image-send-max": {"images": 4, "image_mib": 10, "budget_mib": 128},
    "image-send-max-default": {"images": 4, "image_mib": 10, "allow_receive_failure": True},
    "image-reject": {"images": 4, "image_mib": 10},
    "queue-reject": {"images": 4, "image_mib": 10},
    "incoming-text": {"payload_mib": 2},
    "incoming-array": {"payload_mib": 2},
    "burst-8": {"budget_mib": 8},
    "burst-32": {"budget_mib": 32},
}
METRICS = re.compile(r"peak_queued_bytes=(\d+) peak_records=(\d+) failed=(true|false)")


def kb_fields(text):
    """Parse explicitly kB-valued proc fields; Linux reports KiB here."""
    return {key: int(value) for key, value in re.findall(r"^(\w+):\s+(\d+) kB$", text, re.M)}


def process_memory(pid):
    try:
        root = Path("/proc") / str(pid)
        # Pair the counters with a birth identifier so a reused PID is rejected.
        birth = (root / "stat").read_text().rsplit(")", 1)[1].split()[19]
        status = kb_fields((root / "status").read_text())
        rollup = kb_fields((root / "smaps_rollup").read_text())
        return {"birth": birth, "rss_kib": rollup["Rss"], "hwm_kib": status["VmHWM"]}
    except (FileNotFoundError, ProcessLookupError, KeyError):
        return None


def pressure_frame(size=128 * 1024):
    prefix = b'{"type":"tool_execution_update","toolCallId":"synthetic","partialResult":{"content":[{"type":"text","text":"'
    suffix = b'"}]}}\n'
    if size < len(prefix) + len(suffix):
        raise ValueError("frame size is smaller than the JSON envelope")
    return prefix + b"x" * (size - len(prefix) - len(suffix)) + suffix


def mock_backend():
    """Invoked only by the temporary HIBISCUS_PI launcher; strict LF input."""
    root = Path(os.environ["MEMORY_PROFILE_ROOT"])
    config = json.loads((root / "config.json").read_text())
    name = config["scenario"]
    birth = Path("/proc/self/stat").read_text().rsplit(")", 1)[1].split()[19]
    (root / f"backend-{os.getpid()}").write_text(birth)
    output = sys.stdout.buffer
    turn = 0
    state_count = 0

    def send(record):
        output.write(json.dumps(record, separators=(",", ":")).encode() + b"\n")
        output.flush()

    def settled():
        send({"type": "message_update", "assistantMessageEvent": {
            "type": "text_delta", "delta": f"\nMEMORY_DONE_{turn}"}})
        send({"type": "agent_settled"})

    for line in sys.stdin.buffer:  # bytes iteration splits on LF, not Unicode separators
        command = json.loads(line)
        kind = command["type"]
        if kind in ("get_state", "new_session"):
            data = {}
            if kind == "get_state":
                state_count += 1
                data = {"sessionName": f"MEMORY_STATE_{state_count}"}
            send({"type": "response", "id": command["id"], "success": True, "data": data})
        elif kind == "prompt":
            turn += 1
            rejected = name == "image-reject" or (name == "queue-reject" and turn > 1)
            send({"type": "response", "id": command["id"], "success": not rejected,
                  **({"error": "PROFILE_REJECT"} if rejected else {})})
            if rejected:
                send({"type": "agent_settled"})
            elif name == "queue-reject":
                send({"type": "message_update", "assistantMessageEvent": {"type": "thinking_start"}})
            elif name.startswith("burst-"):
                send({"type": "extension_ui_request", "id": "hold", "method": "confirm",
                      "title": "Memory profile hold", "message": "Synthetic consumer pause", "timeout": 15000})
                deadline = time.monotonic() + 10
                while not (root / "release-burst").exists():
                    if time.monotonic() >= deadline:
                        raise TimeoutError("profile driver did not release burst")
                    time.sleep(0.005)
                try:
                    # Finite producer; EPIPE is the expected explicit-overload signal.
                    frame = pressure_frame()
                    for _ in range(1024):
                        output.write(frame)
                        output.flush()
                        time.sleep(0.001)
                    raise RuntimeError("expected inbox overload did not occur")
                except BrokenPipeError:
                    (root / "burst-closed").touch()
                    # Avoid flushing a failed buffered stream during teardown.
                    os.dup2(os.open(os.devnull, os.O_WRONLY), sys.stdout.fileno())
            elif name.startswith("incoming-"):
                size = config["payload_mib"] * MIB
                payload = (b'"' + b"x" * size + b'"') if name.endswith("text") else (b"[" + b"0," * (size // 2 - 1) + b"0]")
                output.write(b'{"type":"tool_execution_update","toolCallId":"synthetic","partialResult":{"details":' + payload + b'}}\n')
                output.flush()
                del payload
                settled()
            elif name == "long-chat":
                prose = "Some ordinary **bold** prose with `code` and words for the terminal.\n"
                send({"type": "message_update", "assistantMessageEvent": {
                    "type": "text_delta", "delta": prose * (256 * 1024 // len(prose))}})
                settled()
            else:
                # Echo images as a user message, as a real RPC session can do.
                if command.get("images"):
                    send({"type": "message_start", "message": {"role": "user", "content": [
                        {"type": "text", "text": command["message"]}, *command["images"]]}})
                settled()
        del command, line  # Do not keep the previous image request while idle.


class Profile:
    def __init__(self, binary, scenario, config, root):
        self.root, self.scenario = root, scenario
        self.samples, self.phases, self.seen = [], {}, {}
        self.outcome = "completed"
        self.last_sample = 0.0
        self.buffer, self.cursor = b"", 0
        launcher = root / "pi"
        launcher.write_text(f"#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(Path(__file__).resolve()))} --backend\n")
        launcher.chmod(0o755)
        (root / "config.json").write_text(json.dumps(dict(config, scenario=scenario)))
        if config.get("images"):
            # Signature-valid synthetic bytes, not a decoded screenshot benchmark.
            size = config["image_mib"] * MIB
            (root / "image").write_bytes(b"\x89PNG\r\n\x1a\n" + b"\0" * (size - 8))
        for name in ("wl-paste", "xclip", "osascript", "powershell.exe"):
            helper = root / name
            helper.write_text('#!/bin/sh\nfor arg in "$@"; do\n case "$arg" in --list-types|TARGETS) printf "image/png\\n"; exit 0;; esac\ndone\ncat "$MEMORY_PROFILE_ROOT/image"\n')
            helper.chmod(0o755)
        # Do not inherit credentials, real Pi settings, or clipboard-selection env.
        env = {"PATH": str(root) + os.pathsep + os.environ.get("PATH", os.defpath),
               "HOME": str(root), "XDG_CACHE_HOME": str(root / "cache"),
               "XDG_DATA_HOME": str(root / "data"), "PI_CODING_AGENT_DIR": str(root / "agent"),
               "PI_CODING_AGENT_SESSION_DIR": str(root / "sessions"),
               "TERM": "xterm-256color", "LANG": "C.UTF-8", "WAYLAND_DISPLAY": "fixture",
               "HIBISCUS_PI": str(launcher), "HIBISCUS_NO_UPDATE_CHECK": "1",
               "HIBISCUS_RPC_METRICS": "1", "HIBISCUS_RPC_BUFFER_MIB": str(config.get("budget_mib", 64)),
               "HIBISCUS_RPC_MAX_RECORDS": "4096", "MEMORY_PROFILE_ROOT": str(root)}
        self.master, slave = os.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        self.stderr = (root / "stderr").open("wb")
        def setup():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
        try:
            self.child = subprocess.Popen([str(binary)], stdin=slave, stdout=slave, stderr=self.stderr,
                                          cwd=root, env=env, preexec_fn=setup)
        finally:
            os.close(slave)
        os.set_blocking(self.master, False)
        self.binary = binary

    def sample(self, force=False):
        now = time.monotonic()
        if not force and now - self.last_sample < 0.01:
            return
        self.last_sample = now
        actors = [("hibiscus", self.child.pid)] + [("backend", int(p.name.split("-")[1])) for p in self.root.glob("backend-*")]
        for actor, pid in actors:
            try:
                if actor == "hibiscus" and Path(os.readlink(f"/proc/{pid}/exe")) != self.binary:
                    continue  # Exclude the pre-exec Python child image.
                memory = process_memory(pid)
            except FileNotFoundError:
                continue
            if memory is None:
                continue
            key = (actor, pid)
            if key in self.seen and self.seen[key] != memory["birth"]:
                raise RuntimeError("PID reused during profiling")
            self.seen[key] = memory.pop("birth")
            self.samples.append(dict(actor=actor, time=now, **memory))

    def pump(self, timeout=0.01):
        self.sample()
        if select.select([self.master], [], [], timeout)[0]:
            try:
                data = os.read(self.master, 65536)
            except OSError as error:
                if error.errno not in (errno.EIO, errno.EAGAIN):
                    raise
                data = b""
            self.buffer += data
            if len(self.buffer) > 1024 * 1024:
                cut = len(self.buffer) - 1024 * 1024
                self.buffer = self.buffer[cut:]
                self.cursor = max(0, self.cursor - cut)

    def wait(self, text, timeout=20):
        return self.wait_any([text], timeout)

    def wait_any(self, texts, timeout=20):
        end = time.monotonic() + timeout
        while True:
            self.pump()
            matches = [(self.buffer.find(text.encode(), self.cursor), text) for text in texts]
            matches = [(at, text) for at, text in matches if at >= 0]
            if matches:
                at, text = min(matches)
                self.cursor = at + len(text.encode())
                return text
            if self.child.poll() is not None or time.monotonic() >= end:
                raise RuntimeError(f"{self.scenario}: failed waiting for {texts!r}; exit={self.child.returncode}")

    def send(self, data):
        os.write(self.master, data)

    def phase(self, name, seconds=0.15):
        start = time.monotonic()
        end = start + seconds
        while time.monotonic() < end:
            self.pump()
        self.sample(force=True)
        values = [s["rss_kib"] for s in self.samples if s["actor"] == "hibiscus" and s["time"] >= start]
        if not values:
            raise RuntimeError(f"No RSS observations for {name}")
        self.phases[name] = {"rss_median_kib": statistics.median(values), "rss_max_kib": max(values), "samples": len(values)}

    def close(self):
        # Pi is in its own process group; clean both owned process groups on error.
        for path in self.root.glob("backend-*"):
            pid = int(path.name.split("-")[1])
            memory = process_memory(pid)
            if memory and path.read_text() == memory["birth"]:
                try:
                    os.killpg(pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
        if self.child.poll() is None:
            try:
                os.killpg(self.child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        self.child.wait(timeout=5)
        os.close(self.master)
        self.stderr.close()

    def finish(self, config):
        self.send(b"/quit\r")
        deadline = time.monotonic() + 10
        while self.child.poll() is None:
            self.pump()
            if time.monotonic() >= deadline:
                raise TimeoutError("Hibiscus did not exit")
        if self.child.returncode != 0:
            raise RuntimeError(f"Unexpected Hibiscus exit {self.child.returncode}")
        metrics = [{"peak_queued_bytes": int(b), "peak_records": int(n), "receive_failed": failed == "true"}
                   for b, n, failed in METRICS.findall((self.root / "stderr").read_text())]
        if not metrics:
            raise RuntimeError("RPC metrics missing; verify the binary supports HIBISCUS_RPC_METRICS")
        budget = config.get("budget_mib", 64) * MIB
        assert all(m["peak_queued_bytes"] <= budget and m["peak_records"] <= 4096 for m in metrics)
        peak = {}
        for actor in ("hibiscus", "backend"):
            points = [s for s in self.samples if s["actor"] == actor]
            if not points:
                raise RuntimeError(f"No {actor} samples")
            peak[actor] = {"sampled_rss_peak_kib": max(s["rss_kib"] for s in points),
                           "observed_vm_hwm_kib": max(s["hwm_kib"] for s in points), "samples": len(points)}
        received_failure = any(m["receive_failed"] for m in metrics)
        expected_failure = self.scenario.startswith("burst-") or self.outcome == "disconnected"
        assert received_failure == expected_failure, "Unexpected receive-failure status"
        return {"scenario": self.scenario, "config": config, "outcome": self.outcome, "processes": peak,
                "phases": self.phases, "rpc_metrics": metrics}


def run(binary, name, config):
    with tempfile.TemporaryDirectory(prefix="hibiscus-memory-") as temp:
        profile = Profile(binary, name, config, Path(temp))
        try:
            profile.wait("Enter send")
            profile.wait("MEMORY_STATE_1")
            profile.phase("idle")
            if name == "long-chat":
                for n in range(1, config["turns"] + 1):
                    profile.send(b"synthetic\r")
                    profile.wait(f"MEMORY_DONE_{n}")
                    profile.wait(f"MEMORY_STATE_{n + 1}")
                    profile.phase(f"turn_{n}", 0.05)
                profile.send(b"/new\r")
                profile.wait(f"MEMORY_STATE_{config['turns'] + 2}")
                profile.phase("after_new")
            elif config.get("images"):
                if name == "queue-reject":
                    profile.send(b"first\r")
                    profile.wait("Thinking…")
                for n in range(1, config["images"] + 1):
                    profile.send(b"\x16")
                    profile.wait(f"{n} image(s) attached")
                    profile.phase(f"attached_{n}")
                profile.send(b"images\r")
                if name == "queue-reject":
                    profile.wait("Pi rejected queued message")
                elif name == "image-reject":
                    profile.wait("PROFILE_REJECT")
                elif config.get("allow_receive_failure"):
                    result = profile.wait_any(["MEMORY_DONE_1", "Pi disconnected"])
                    if result == "MEMORY_DONE_1":
                        result = profile.wait_any(["MEMORY_STATE_2", "Pi disconnected"])
                    if result == "Pi disconnected":
                        profile.outcome = "disconnected"
                else:
                    profile.wait("MEMORY_DONE_1")
                if not config.get("allow_receive_failure"):
                    profile.wait("MEMORY_STATE_2")
                profile.phase("after_result")
                if name in ("image-reject", "queue-reject"):
                    if name == "queue-reject":
                        profile.send(b"\x03")  # Clear live copy, retain explicit restore slot.
                        profile.phase("after_clear_live")
                    profile.send(b"/restore\r")
                    profile.wait(f"{config['images']} image(s) attached")
                    profile.phase("restored")
                    profile.send(b"\x03")
                    profile.phase("after_clear_restored")
            elif name.startswith("burst-"):
                profile.send(b"burst\r")
                profile.wait("Memory profile hold")
                profile.phase("modal_before_burst")
                (profile.root / "release-burst").touch()
                deadline = time.monotonic() + 10
                while not (profile.root / "burst-closed").exists():
                    profile.pump()
                    if time.monotonic() >= deadline:
                        raise TimeoutError("Expected EPIPE after overload")
                profile.phase("overloaded_modal")
                profile.send(b"\x1b")
                profile.wait("Pi disconnected")
                profile.outcome = "disconnected"
                profile.phase("disconnected")
            elif name.startswith("incoming-"):
                profile.send(b"incoming\r")
                profile.wait("MEMORY_DONE_1")
                profile.wait("MEMORY_STATE_2")
                profile.phase("after_result")
            return profile.finish(config)
        finally:
            profile.close()


def main():
    if sys.argv[1:] == ["--backend"]:
        mock_backend()
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/hibiscus"))
    parser.add_argument("--scenario", choices=["all", *SCENARIOS], default="all")
    parser.add_argument("--repeats", type=int, choices=range(1, 11), default=1)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if sys.platform != "linux" or not Path("/proc/self/smaps_rollup").exists():
        parser.error("Requires Linux /proc with readable smaps_rollup; no macOS RSS approximation is substituted")
    binary = args.binary.resolve(strict=True)
    names = list(SCENARIOS) if args.scenario == "all" else [args.scenario]
    results = []
    report = {"schema": 1, "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
              "measurement": "Linux smaps_rollup RSS sampled at ~10 ms; observed per-PID VmHWM; KiB; no child totals",
              "results": results}
    if args.output:
        args.output.write_text(json.dumps(dict(report, complete=False), indent=2) + "\n")
    for name in names:
        for repeat in range(args.repeats):
            if hashlib.sha256(binary.read_bytes()).hexdigest() != report["binary_sha256"]:
                raise RuntimeError("Binary changed during profiling; rebuild and restart the experiment")
            print(f"Profiling {name} ({repeat + 1}/{args.repeats})", file=sys.stderr, flush=True)
            results.append(dict(run(binary, name, SCENARIOS[name]), repeat=repeat + 1))
            # Preserve completed experiments if a later scenario fails.
            if args.output:
                args.output.write_text(json.dumps(dict(report, complete=False), indent=2) + "\n")
    report["complete"] = True
    text = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
