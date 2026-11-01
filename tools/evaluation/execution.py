"""Bounded Unix subprocesses and canonical native-session evidence."""
import json
import os
import signal
import subprocess
import time

from .spec import InvalidSpec, inventory, materialize, read_file

OUTPUT_LIMIT = 8 * 1024 * 1024
AGENT_CREDENTIAL_ENV = "HIBISCUS_EVAL_PROVIDER_API_KEY"


def environment(root, provider_key=None):
    # Never inherit model overrides, proxy settings, other provider keys or homes.
    env = {"PATH": os.defpath, "HOME": str(root / "user"),
           "HIBISCUS_HOME": str(root / "app"), "HIBISCUS_NO_UPDATE_CHECK": "1",
           "NO_COLOR": "1", "TERM": "dumb", "LANG": "C.UTF-8",
           "PYTHONDONTWRITEBYTECODE": "1"}
    if provider_key is not None:
        env[AGENT_CREDENTIAL_ENV] = provider_key
    return env


def process(command, cwd, env, timeout, logs):
    started = time.monotonic()
    result = {"exitCode": None, "error": None, "durationMs": 0}
    with logs.with_suffix(".stdout").open("wb") as stdout, logs.with_suffix(".stderr").open("wb") as stderr:
        try:
            child = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                     stdout=stdout, stderr=stderr, start_new_session=True)
        except OSError:
            result["error"] = "spawn_failed"
            result["durationMs"] = round((time.monotonic() - started) * 1000)
            return result
        try:
            while child.poll() is None:
                if time.monotonic() - started >= timeout:
                    result["error"] = "timeout"
                    break
                if os.fstat(stdout.fileno()).st_size + os.fstat(stderr.fileno()).st_size > OUTPUT_LIMIT:
                    result["error"] = "output_limit"
                    break
                time.sleep(0.01)
        finally:
            # Also terminate descendants left behind by an already-exited leader.
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait(timeout=5)
        result["exitCode"] = child.returncode
        if os.fstat(stdout.fileno()).st_size + os.fstat(stderr.fileno()).st_size > OUTPUT_LIMIT:
            result["error"] = "output_limit"
    result["durationMs"] = round((time.monotonic() - started) * 1000)
    if result["exitCode"] != 0 and result["error"] is None:
        result["error"] = "nonzero_exit"
    return result


def prepare(root, files, variant, config):
    root.mkdir(mode=0o700)
    for directory in ("user", "app", "workspace"):
        (root / directory).mkdir(mode=0o700)
    materialize(files, root / "workspace")
    app = root / "app"
    (app / "config.json").write_text(json.dumps(config))
    (app / "config.json").chmod(0o600)
    (app / "AGENTS.md").write_bytes(variant.instructions)
    for name, images in variant.extensions:
        materialize(images, app / "extensions" / name)


def session_evidence(home, model, prompt):
    """Interpret format v1 only. Never score raw assistant prose as task success."""
    empty = {"valid": False, "errors": [], "tools": [], "tokens": None,
             "sessionDigest": None, "extensionReport": None}
    try:
        import hashlib
        paths = list((home / "sessions").glob("*.jsonl"))
        if len(paths) != 1:
            raise InvalidSpec("expected_one_session")
        content = read_file(paths[0], 64 * 1024 * 1024)
        records = [json.loads(line) for line in content.splitlines()]
        head, *records = records
        if (head.get("type") != "session" or head.get("format") != "hibiscus"
                or head.get("version") != 1 or head.get("model") != model
                or head.get("thinking") != "off"
                or head.get("cwd") != str((home.parent / "workspace").resolve())):
            raise InvalidSpec("unsupported_or_mismatched_session")
        usage = head["usage"]
        tokens = {}
        for key in ("input", "output", "cacheRead"):
            value = usage.get(key)
            if value is not None and (type(value) is not int or value < 0):
                raise InvalidSpec("invalid_usage")
            tokens[key] = value
        empty["tokens"] = tokens
        empty["sessionDigest"] = hashlib.sha256(content).hexdigest()
        pending = {}
        tools = []
        empty["tools"] = tools
        users = []
        final = None
        extension_report = None
        for record in records:
            if record.get("type") != "message":
                raise InvalidSpec("unsupported_session_record")
            message = record["message"]
            role = message["role"]
            if role == "user":
                users.append("".join(b["text"] for b in message["content"] if b["type"] == "text"))
            elif role == "assistant":
                final = message.get("stopReason")
                for block in message["content"]:
                    if block["type"] == "toolCall":
                        if block["id"] in pending:
                            raise InvalidSpec("duplicate_pending_call")
                        pending[block["id"]] = block["name"]
            elif role == "toolResult":
                name = pending.pop(message["toolCallId"], None)
                if name != message["toolName"] or type(message["isError"]) is not bool:
                    raise InvalidSpec("uncorrelated_tool_result")
                details = message.get("details", {})
                exit_code = details.get("exitCode") if isinstance(details, dict) else None
                tools.append({"name": name, "isError": message["isError"],
                              "exitCode": exit_code if type(exit_code) is int else None})
                if name == "list_extensions" and not message["isError"]:
                    extension_report = details
                    empty["extensionReport"] = details
            else:
                raise InvalidSpec("unknown_message_role")
        if pending or users != [prompt] or final != "stop":
            raise InvalidSpec("incomplete_or_mismatched_run")
        return {"valid": True, "errors": [], "tools": tools, "tokens": tokens,
                "sessionDigest": hashlib.sha256(content).hexdigest(),
                "extensionReport": extension_report}
    except (OSError, ValueError, KeyError, TypeError, IndexError, AttributeError):
        empty["errors"] = ["missing_invalid_or_incomplete_session"]
        return empty


def grade(task, root, env, timeout):
    # Grading code appears only AFTER the agent process and descendants stop.
    import sys
    path = root / "verify.py"
    path.write_bytes(task.grader)
    execution = process([sys.executable, "-I", "-B", str(path), str(root / "workspace")],
                        root, env, timeout, root / "grade")
    try:
        after = {f.path: f.identity() for f in inventory(root / "workspace")}
        before = {f.path: f.identity() for f in task.files}
        changed = sorted(p for p in before.keys() | after.keys() if before.get(p) != after.get(p))
        return {"checksPassed": execution["error"] is None,
                "requiredFilesChanged": all(p in changed for p in task.changed),
                "preservedFilesUnchanged": all(p not in changed for p in task.preserved),
                "changedPaths": changed, "checkExecution": execution}
    except (InvalidSpec, OSError):
        return {"checksPassed": False, "requiredFilesChanged": False,
                "preservedFilesUnchanged": False, "changedPaths": [],
                "checkExecution": execution, "error": "invalid_workspace_inventory"}
