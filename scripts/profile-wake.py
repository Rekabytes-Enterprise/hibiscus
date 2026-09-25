#!/usr/bin/env python3
"""Optional Linux PTY key-to-output latency probe with an isolated fake Pi.

Build: cargo build --release --locked
Run: python3 scripts/profile-wake.py --samples 60 --output /tmp/hibiscus-wake.json
No real models, settings, credentials or desktop clipboard are accessed.
Measurements include PTY scheduling and are not CI latency gates.
"""
import argparse
import errno
import fcntl
import json
import os
from pathlib import Path
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


def backend():
    # Pi stdout is protocol-only. No provider, session or file operations.
    for line in sys.stdin.buffer:
        command = json.loads(line)
        kind = command["type"]
        reply = {"type": "response", "id": command["id"], "success": True}
        if kind == "get_state":
            reply["data"] = {}
        print(json.dumps(reply), flush=True)
        if kind == "prompt":
            print('{"type":"message_update","assistantMessageEvent":{"type":"thinking_start"}}', flush=True)
        elif kind == "abort":
            print('{"type":"agent_settled"}', flush=True)


class Probe:
    def __init__(self, binary, root):
        launcher = root / "pi"
        launcher.write_text(f"#!/bin/sh\nexec {shlex.quote(sys.executable)} {shlex.quote(str(Path(__file__).resolve()))} --backend\n")
        launcher.chmod(0o755)
        self.master, slave = os.openpty()
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
        self.stderr = (root / "stderr").open("wb")
        # Isolate all Pi/session settings; child never contacts a real provider.
        env = {"PATH": str(root) + os.pathsep + os.environ.get("PATH", os.defpath),
               "HOME": str(root), "PI_CODING_AGENT_DIR": str(root / "agent"),
               "PI_CODING_AGENT_SESSION_DIR": str(root / "sessions"),
               "XDG_CACHE_HOME": str(root / "cache"), "XDG_DATA_HOME": str(root / "data"),
               "LANG": "C.UTF-8", "TERM": "xterm-256color", "NO_COLOR": "1",
               "HIBISCUS_PI": str(launcher), "HIBISCUS_NO_UPDATE_CHECK": "1"}
        def setup():
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
        try:
            self.child = subprocess.Popen([str(binary)], stdin=slave, stdout=slave,
                                          stderr=self.stderr, cwd=root, env=env, preexec_fn=setup)
        finally:
            os.close(slave)
        os.set_blocking(self.master, False)
        self.output = b""
        self.cursor = 0

    def pump(self, timeout):
        if select.select([self.master], [], [], timeout)[0]:
            try:
                data = os.read(self.master, 65536)
            except OSError as exc:
                if exc.errno not in (errno.EIO, errno.EAGAIN):
                    raise
                data = b""
            self.output += data
            if len(self.output) > 512 * 1024:
                cut = len(self.output) - 512 * 1024
                self.output = self.output[cut:]
                self.cursor = max(0, self.cursor - cut)

    def wait(self, marker, timeout=10):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            self.pump(0.001)
            position = self.output.find(marker, self.cursor)
            if position >= 0:
                self.cursor = position + len(marker)
                return time.perf_counter()
            if self.child.poll() is not None:
                break
        raise RuntimeError(f"Hibiscus did not emit expected marker (child={self.child.poll()})")

    def send(self, bytes_):
        self.pump(0)
        self.cursor = len(self.output)
        start = time.perf_counter()
        if os.write(self.master, bytes_) != len(bytes_):
            raise RuntimeError("incomplete PTY input")
        return start

    def close(self):
        if self.child.poll() is None:
            try:
                os.killpg(self.child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
        self.child.wait(timeout=5)
        os.close(self.master)
        self.stderr.close()


def run(binary, count):
    with tempfile.TemporaryDirectory(prefix="hibiscus-wake-") as tmp:
        probe = Probe(binary, Path(tmp))
        try:
            probe.wait(b"Enter send")
            probe.send(b"start\r")
            probe.wait("Thinking…".encode())
            durations = []
            # Change the arrival phase relative to the existing 40 ms poll.
            for index in range(1, count + 1):
                time.sleep((3, 11, 19, 27, 37)[index % 5] / 1000)
                started = probe.send(b"a")
                ended = probe.wait("❯ ".encode() + b"a" * index)
                durations.append((ended - started) * 1000)
            probe.send(b"\x1b")
            probe.wait(b"Stopped. You can keep chatting.")
            probe.send(b"\x03/quit\r")
            end = time.monotonic() + 5
            while probe.child.poll() is None and time.monotonic() < end:
                probe.pump(0.01)
            if probe.child.returncode != 0:
                raise RuntimeError("mock Pi chat did not exit cleanly")
            durations.sort()
            return {"samples": count, "median_ms": round(statistics.median(durations), 3),
                    "p95_ms": round(durations[int((count - 1) * 0.95)], 3),
                    "min_ms": round(durations[0], 3), "max_ms": round(durations[-1], 3)}
        finally:
            probe.close()


def main():
    if sys.argv[1:] == ["--backend"]:
        backend()
        return
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/hibiscus"))
    parser.add_argument("--samples", type=int, default=60)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if sys.platform != "linux":
        parser.error("requires Linux controlling PTY; macOS results are not inferred")
    if not 5 <= args.samples <= 500:
        parser.error("--samples must be between 5 and 500")
    result = run(args.binary.resolve(strict=True), args.samples)
    text = json.dumps(result, indent=2) + "\n"
    if args.output:
        args.output.write_text(text)
    else:
        print(text, end="")


if __name__ == "__main__":
    main()
