#!/usr/bin/env python3
"""Package or run the real cargo-fuzz binaries. Development-only; no account access."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parent.parent
MAX_INPUT = 131072


def targets():
    result = subprocess.run(
        ["cargo", "metadata", "--manifest-path", str(ROOT / "fuzz/Cargo.toml"),
         "--format-version", "1", "--no-deps", "--offline", "--locked"],
        check=True, stdout=subprocess.PIPE, text=True, timeout=30)
    metadata = json.loads(result.stdout)
    package = next(p for p in metadata["packages"] if p["name"] == "hibiscus-fuzz")
    names = sorted(t["name"] for t in package["targets"] if t["kind"] == ["bin"])
    assert names and all(n.replace("_", "").isalnum() for n in names)
    host = subprocess.check_output(["rustc", "-vV"], text=True, timeout=30)
    host = next(line.split(": ", 1)[1] for line in host.splitlines() if line.startswith("host: "))
    return names, Path(metadata["target_directory"]) / host / "release"


def package(output):
    names, binaries = targets()
    output.mkdir(parents=True, exist_ok=True)
    for name in names:
        binary = binaries / name
        if not binary.is_file() or binary.is_symlink():
            raise ValueError("Missing regular fuzz binary: " + name)
        shutil.copy2(binary, output / name)
        seeds = ROOT / "fuzz/corpus" / name
        with zipfile.ZipFile(output / (name + "_seed_corpus.zip"), "w", zipfile.ZIP_DEFLATED) as archive:
            for seed in sorted(seeds.iterdir()):
                if not seed.is_file() or seed.is_symlink() or seed.stat().st_size > MAX_INPUT:
                    raise ValueError("Invalid curated seed")
                archive.write(seed, seed.name)
        (output / (name + ".options")).write_text(
            "[libfuzzer]\nmax_len=131072\ntimeout=5\nrss_limit_mb=2048\n")
    print(json.dumps({"packaged_targets": names}))


def smoke(seconds, artifacts, binary_directory):
    names, built = targets()
    binaries = binary_directory or built
    artifacts.mkdir(parents=True, exist_ok=True)
    # Provider/account variables never reach fuzzers. Fixtures are synthetic.
    env = {k: os.environ[k] for k in ("PATH", "TMPDIR", "SYSTEMROOT") if k in os.environ}
    with tempfile.TemporaryDirectory(prefix="hibiscus-fuzz-") as temporary:
        env.update(HOME=temporary, HIBISCUS_HOME=str(Path(temporary) / ".hibiscus"),
                   HIBISCUS_NO_UPDATE_CHECK="1")
        for name in names:
            corpus = Path(temporary) / name
            shutil.copytree(ROOT / "fuzz/corpus" / name, corpus)
            result = subprocess.run(
                [str((binaries / name).resolve()), str(corpus), "-max_len=131072",
                 "-timeout=5", "-rss_limit_mb=2048", "-print_final_stats=1",
                 "-max_total_time=" + str(seconds), "-artifact_prefix=" + str(artifacts.resolve()) + "/"],
                env=env, timeout=seconds + 45)
            if result.returncode:
                raise SystemExit(result.returncode)
    print(json.dumps({"passed_targets": names, "seconds_per_target": seconds}))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    pack = commands.add_parser("package")
    pack.add_argument("--output", type=Path, required=True)
    run = commands.add_parser("smoke")
    run.add_argument("--seconds", type=int, choices=range(1, 601), default=10)
    run.add_argument("--artifacts", type=Path, default=ROOT / "fuzz/artifacts")
    run.add_argument("--binary-directory", type=Path)
    args = parser.parse_args()
    if args.command == "package":
        package(args.output)
    else:
        smoke(args.seconds, args.artifacts, args.binary_directory)


if __name__ == "__main__":
    main()
