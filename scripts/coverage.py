#!/usr/bin/env python3
"""Measure Rust source coverage locally without a coverage service or credentials."""
import argparse
import fcntl
import json
import os
from pathlib import Path
import platform
import subprocess
import sys
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
TOOL_VERSION = "0.9.1"
TOOLCHAIN = "nightly-2026-10-01"
# Dedicated test files are also excluded by cargo-llvm-cov's default filters.
# Inline unit-test code remains in the source-file denominator; do not call this
# production-only coverage or use it to attest the Best Practices 80% criterion.
# Lines of the test log shown when the tests fail.
LOG_TAIL = 80
IGNORE = r"/tests/|/src/fuzzing.rs$|/src/[^ ]*/fuzzing.rs$"
METRICS = ("lines", "regions", "functions", "branches")


def summary(report):
    if report.get("type") != "llvm.coverage.json.export":
        raise ValueError("Expected an LLVM coverage export")
    tool = report.get("cargo_llvm_cov", {})
    if tool.get("version") != TOOL_VERSION:
        raise ValueError("Coverage tool version does not match the pinned version")
    if Path(tool.get("manifest_path", "")).resolve() != ROOT / "Cargo.toml":
        raise ValueError("Coverage report belongs to another manifest")
    files = {}
    totals = {metric: {"count": 0, "covered": 0} for metric in METRICS}
    for unit in report["data"]:
        for item in unit["files"]:
            path = Path(item["filename"]).resolve().relative_to(ROOT).as_posix()
            if not path.startswith("src/") or path in files:
                raise ValueError("Unexpected or repeated source file: " + path)
            metrics = {}
            for metric in METRICS:
                value = item["summary"][metric]
                count, covered = value["count"], value["covered"]
                if (type(count) is not int or type(covered) is not int
                        or not 0 <= covered <= count):
                    raise ValueError("Invalid coverage counters")
                metrics[metric] = {"count": count, "covered": covered,
                                   "percent": round(100 * covered / count, 2) if count else None}
                totals[metric]["count"] += count
                totals[metric]["covered"] += covered
            files[path] = metrics
    if not files or not totals["branches"]["count"]:
        raise ValueError("Empty report or missing branch instrumentation")
    if not files.get("src/main.rs", {}).get("lines", {}).get("covered", 0):
        raise ValueError("Native binary entry point was not covered")
    for value in totals.values():
        value["percent"] = round(100 * value["covered"] / value["count"], 2) if value["count"] else None
    return {"scope": "Rust source files, including inline unit-test code",
            "totals": totals, "files": dict(sorted(files.items()))}


def run(output):
    output = output.resolve()
    output.relative_to(ROOT / "target")
    # A failed run never reuses a previous success report.
    output.mkdir(parents=True, mode=0o700, exist_ok=False)
    build = ROOT / "target/security-coverage"
    env = {key: os.environ[key] for key in (
        "PATH", "HOME", "CARGO_HOME", "RUSTUP_HOME", "TMPDIR", "LANG", "LC_ALL",
        "DEVELOPER_DIR", "SDKROOT") if key in os.environ}
    env.update(CARGO_TARGET_DIR=str(build), CARGO_INCREMENTAL="0", TERM="dumb",
               HIBISCUS_NO_UPDATE_CHECK="1", PYTHONDONTWRITEBYTECODE="1")
    cargo = ["cargo", "+" + TOOLCHAIN, "llvm-cov"]

    def call(arguments):
        return subprocess.check_output(arguments, cwd=ROOT, env=env, text=True).strip()

    version = call(cargo + ["--version"])
    if version != "cargo-llvm-cov " + TOOL_VERSION:
        raise ValueError("Install cargo-llvm-cov " + TOOL_VERSION)
    compiler = call(["rustc", "+" + TOOLCHAIN, "-Vv"])
    # One build/profile store at a time. cargo-llvm-cov cleans old profiles before
    # each full test run; report-only exports below consume that same run.
    with (ROOT / "target/coverage.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        with (output / "tests.log").open("w") as log:
            tests = subprocess.run(cargo + ["--locked", "--package", "hibiscus", "--features", "fuzzing",
                                           "--branch", "--json", "--output-path", str(output / "coverage.json"),
                                           "--ignore-filename-regex", IGNORE], cwd=ROOT, env=env,
                                   stdout=log, stderr=subprocess.STDOUT)
        if tests.returncode:
            # The log is not uploaded, so show which test failed before giving up.
            print("".join((output / "tests.log").read_text(errors="replace").splitlines(True)[-LOG_TAIL:]),
                  file=sys.stderr)
            tests.check_returncode()
        measured = summary(json.loads((output / "coverage.json").read_text()))
        subprocess.run(cargo + ["report", "--branch", "--html", "--output-dir", str(output),
                               "--ignore-filename-regex", IGNORE], cwd=ROOT, env=env, check=True)
    measured.update(formatVersion=1, measuredAt=datetime.now(timezone.utc).isoformat(),
                    tool=version, toolchain=TOOLCHAIN, compiler=compiler,
                    system=platform.system(), architecture=platform.machine(),
                    sourceCommit=call(["git", "rev-parse", "HEAD"]),
                    workingTreeModified=bool(call(["git", "status", "--porcelain"])),
                    limitations=["Includes inline unit-test code; dedicated test/fuzz-adapter files are excluded.",
                                 "No Python, shell, dependency, C/FFI or other-platform coverage is measured.",
                                 "Killed processes may not flush profiles. No live provider is used.",
                                 "Branch instrumentation is experimental. This is not an 80% badge attestation."])
    (output / "summary.json").write_text(json.dumps(measured, indent=2) + "\n")
    print(json.dumps({"output": str(output), "totals": measured["totals"]}, indent=2))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "target/coverage/local")
    args = parser.parse_args()
    run(args.output)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print("Coverage failed: " + str(error), file=sys.stderr)
        raise SystemExit(1)
