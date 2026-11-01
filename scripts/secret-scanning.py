#!/usr/bin/env python3
"""Pinned development-only secret detector. Never prints matched values."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
import tarfile
import tempfile
import urllib.request

VERSION = "8.30.1"
ARCHIVES = {
    ("Linux", "x86_64"): ("linux_x64", "551f6fc83ea457d62a0d98237cbad105af8d557003051f41f3e7ca7b3f2470eb"),
    ("Linux", "aarch64"): ("linux_arm64", "e4a487ee7ccd7d3a7f7ec08657610aa3606637dab924210b3aee62570fb4b080"),
    ("Darwin", "arm64"): ("darwin_arm64", "b40ab0ae55c505963e365f271a8d3846efbc170aa17f2607f13df610a9aeb6a5"),
    ("Darwin", "x86_64"): ("darwin_x64", "dfe101a4db2255fc85120ac7f3d25e4342c3c20cf749f2c20a18081af1952709"),
}
ROOT = Path(__file__).resolve().parents[1]


class ScanError(ValueError):
    pass


def environment(home):
    env = {key: os.environ[key] for key in
           ("PATH", "TMPDIR", "HTTPS_PROXY", "HTTP_PROXY", "ALL_PROXY", "NO_PROXY",
            "SSL_CERT_FILE", "SSL_CERT_DIR") if key in os.environ}
    env.update(HOME=str(home), GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
               GIT_TERMINAL_PROMPT="0", GIT_OPTIONAL_LOCKS="0", NO_COLOR="1")
    return env


def verify_version(scanner):
    with tempfile.TemporaryDirectory(prefix="hibiscus-scanner-version-") as home:
        result = subprocess.run([str(scanner), "version"], env=environment(home),
                                capture_output=True, timeout=30)
    if result.returncode or result.stdout.strip() != VERSION.encode():
        raise ScanError("Expected the pinned Gitleaks version")


def unpack_verified(data, expected, directory):
    if len(data) > 32 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != expected:
        raise ScanError("Scanner download checksum mismatch")
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        member = archive.getmember("gitleaks")
        if not member.isfile() or not 0 < member.size <= 64 * 1024 * 1024:
            raise ScanError("Invalid scanner executable")
        content = archive.extractfile(member).read()
    # A new directory prevents reusing or overwriting an untrusted executable.
    directory.mkdir(mode=0o700, exist_ok=False)
    executable = directory / "gitleaks"
    executable.write_bytes(content)
    executable.chmod(0o700)
    verify_version(executable)
    return executable


def install(directory):
    selection = ARCHIVES.get((platform.system(), platform.machine()))
    if selection is None:
        raise ScanError("Unsupported scanner platform")
    target, digest = selection
    url = f"https://github.com/gitleaks/gitleaks/releases/download/v{VERSION}/gitleaks_{VERSION}_{target}.tar.gz"
    with urllib.request.urlopen(url, timeout=60) as response:
        data = response.read(32 * 1024 * 1024 + 1)
    return unpack_verified(data, digest, directory)


def scan(scanner, source, mode):
    verify_version(scanner)
    with tempfile.TemporaryDirectory(prefix="hibiscus-secret-scan-") as temporary:
        folder = Path(temporary)
        env = environment(folder)
        if mode == "git":
            shallow = subprocess.run(["git", "rev-parse", "--is-shallow-repository"],
                                     cwd=source, env=env, capture_output=True, timeout=30)
            if shallow.returncode or shallow.stdout.strip() != b"false":
                raise ScanError("History scanning requires a full Git checkout")
        report = folder / "report.json"
        ignore = folder / "empty-ignore"
        ignore.write_text("")
        command = [str(scanner), mode, str(source), "--config", str(ROOT / ".gitleaks.toml"),
                   "--redact=100", "--no-banner", "--no-color", "--ignore-gitleaks-allow",
                   "--gitleaks-ignore-path", str(ignore), "--exit-code=1", "--timeout=600",
                   "--max-decode-depth=2", "--report-format=json", "--report-path", str(report)]
        if mode == "git":
            command.append("--log-opts=--all --full-history -m --no-ext-diff --no-textconv")
        result = subprocess.run(command, cwd=source, env=env, capture_output=True, timeout=660)
        if result.returncode not in (0, 1) or not report.is_file() or report.stat().st_size > 32 * 1024 * 1024:
            raise ScanError("Scanner failed; no clean result is available")
        findings = json.loads(report.read_bytes())
        if not isinstance(findings, list) or bool(findings) != (result.returncode == 1):
            raise ScanError("Inconsistent scanner report")
        summaries = []
        for finding in findings:
            if not isinstance(finding, dict) or finding.get("Secret") != "REDACTED":
                raise ScanError("Scanner report did not redact the secret")
            summaries.append({key: finding.get(key) for key in
                              ("RuleID", "File", "StartLine", "Commit")})
        return summaries


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    setup = commands.add_parser("install")
    setup.add_argument("--directory", type=Path, required=True)
    for mode in ("git", "dir"):
        command = commands.add_parser(mode)
        command.add_argument("--scanner", type=Path, required=True)
        command.add_argument("--source", type=Path, default=ROOT)
    args = parser.parse_args()
    try:
        if args.command == "install":
            print(install(args.directory.resolve()))
            return 0
        findings = scan(args.scanner.resolve(), args.source.resolve(), args.command)
        print(json.dumps({"scanner": VERSION, "mode": args.command,
                          "finding_count": len(findings), "locations": findings[:20]}))
        return 1 if findings else 0
    except (ScanError, OSError, subprocess.SubprocessError, tarfile.TarError, KeyError, json.JSONDecodeError) as error:
        # Do not echo subprocess stderr, provider values or malformed report bytes.
        print(str(error) if isinstance(error, ScanError) else "Secret scan could not complete", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
