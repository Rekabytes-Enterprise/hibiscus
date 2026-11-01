#!/bin/bash
set -euo pipefail
cd "$SRC/hibiscus"
rustc --version
cargo fuzz --version
# cargo-fuzz has no --locked flag. Validate before building and refuse any
# lockfile mutation before packaging or executing the resulting fuzz binaries.
cargo fetch --locked --manifest-path fuzz/Cargo.toml
cp Cargo.lock "$WORK/hibiscus-fuzz.lock"
cargo fuzz build -O --debug-assertions
cmp Cargo.lock "$WORK/hibiscus-fuzz.lock"
python3 scripts/fuzzing.py package --output "$OUT"
