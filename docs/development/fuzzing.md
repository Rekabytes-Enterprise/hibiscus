# Native boundary fuzzing

Six development-only libFuzzer targets exercise the production extension manifest
and reply decoders, native RPC JSON/line framing, provider SSE/event parsing,
OAuth redirect validation and extension/checkpoint relative paths. Inputs are
limited to 128 KiB. Production limits remain in their owners, including the
64 KiB manifest/reply, 16 KiB OAuth callback and 256/512-byte path limits.

These adapters never start the runtime, discover extensions, run commands, open
OAuth listeners, read credentials, contact providers or mutate files. Path
fuzzing covers the pure lexical validators. Existing checkpoint tests cover
filesystem links, restore conflicts and recovery separately. This is not an
execution sandbox or a claim to cover every filesystem race.

## Reproduce locally

The native application uses stable Rust. Fuzzing alone needs nightly, a C++
compiler and cargo-fuzz. The locally verified tools are nightly-2026-10-01 and
cargo-fuzz 0.13.2. Install them without changing the default Rust toolchain:

```sh
rustup toolchain install nightly-2026-10-01 --profile minimal
cargo install cargo-fuzz --version 0.13.2 --locked
cargo +nightly-2026-10-01 fuzz build -O --debug-assertions
python3 scripts/fuzzing.py smoke --seconds 15
```

`fuzz/Cargo.toml` is the target inventory used by the build and smoke scripts.
Both lockfiles are tracked; the version-bump helper updates the native package
entry in each. cargo-fuzz has no `--locked` build option. The container build
validates with `cargo fetch --locked` and refuses lock changes before packaging.

For one input or a longer focused run:

```sh
cargo +nightly-2026-10-01 fuzz run provider_stream fuzz/corpus/provider_stream/mixed-delimiters
cargo +nightly-2026-10-01 fuzz run provider_stream -- -max_total_time=60 -max_len=131072 -timeout=5 -rss_limit_mb=2048
```

The smoke script copies curated seeds into disposable corpus directories and
passes a fresh home and filtered environment to each real fuzz binary. Generated
corpus, builds and failures stay ignored locally. Retain a failure, reproduce
with the same target/toolchain, reduce it with `cargo fuzz tmin TARGET INPUT`,
and add the smallest meaningful ordinary regression test before changing code.
Never copy user conversations, real credentials or private files into seeds.

## CI and artifacts

The Native fuzzing workflow runs on PRs and pushes to dev/main, on a daily
schedule and by explicit dispatch. It uses the official ClusterFuzzLite build
and run entry points pinned by container digest. Its Rust builder is also pinned
by digest, fixing the included Rust, LLVM and cargo-fuzz snapshot. The workflow
has only repository/action/PR read permissions, no account/provider secrets,
no persisted checkout credentials, and no release, OIDC or attestation grant.
No storage repository, paid service or OSS-Fuzz enrollment is configured.

The PR/push campaign has a 60-second fuzzing budget; scheduled/manual campaigns
have 600 seconds, divided between the six targets by ClusterFuzzLite. Each input
has a five-second timeout, a 2 GiB RSS limit and a 128 KiB maximum length. The job
has a separate 30-minute wall limit. All targets stay enabled, including those
unaffected by the current diff. Crashes, timeouts and memory failures fail the
campaign; report artifacts are retained for five days. The existing required CI
also replays the curated seeds through the feature-gated adapters.

## Integration design

`src/lib.rs` owns the application's existing private modules and `run()`.
`src/main.rs` only calls that entry point and formats a top-level error. The
`fuzzing` feature adds bounded public byte-input entry points; private adapters
stay alongside their production owners. The separate fuzz package alone adds
libfuzzer-sys, so normal builds have no fuzzing dependency or new command.

A separate parser crate was considered. It would move provider state, manifest
schema ownership and internal representations into a new public contract.
Keeping these owners private and adding the library entry point requires fewer
changes and avoids copied parsers or source-path imports in the harnesses.

## Findings and limits

The real sanitized provider fuzzer reproduced two defects in the original code.
Mixed LF/CRLF SSE frames were grouped differently depending on network chunks;
the decoder now selects the earliest wire delimiter and drains consumed bytes
once. A scalar Anthropic content block followed by a text delta caused a panic;
block shape and matching text deltas now return normal errors. Both reduced
cases failed before the fixes and pass as ordinary provider regression tests.
The original synthetic seeds remain in the curated corpus.

Successful compilation and a clean short campaign do not establish absence of
vulnerabilities. Hosted ClusterFuzzLite operation and Scorecard recognition must
be checked from actual completed runs before claiming either one. Scorecard's
integration heuristic is separate from the observed parser test coverage.
