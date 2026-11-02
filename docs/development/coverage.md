# Rust source and branch coverage

Coverage shows which source locations the tests executed. It helps find missing
tests; it does not establish that assertions are correct or that the program is
secure. Keep the ordinary stable-toolchain CI and sanitized fuzzing alongside it.

## Run locally

Install the development-only tools. They are not Hibiscus runtime dependencies:

```sh
rustup toolchain install nightly-2026-10-01 --profile minimal --component llvm-tools-preview
cargo install cargo-llvm-cov --version 0.9.1 --locked
python3 tests/coverage.py
python3 scripts/coverage.py --output target/coverage/local
```

The output directory must be new and inside `target/`. Use another name for the
next run. The runner verifies the tool version, serializes access to its separate
`target/security-coverage` build directory and lets cargo-llvm-cov clear previous
profiles before testing. Inherited coverage flags, provider keys and proxy
overrides do not reach the measured suite. A failed command stops the run without
producing a success summary or uploading an old result.

On macOS, use a matching installed Xcode/SDK through `DEVELOPER_DIR` and `SDKROOT`
if the default command-line linker cannot read your SDK. The script preserves
those build settings without changing the system-wide developer selection.

Open `target/coverage/local/html/index.html` for annotated source. `coverage.json`
is the full LLVM export; `summary.json` contains recomputed file/total counters,
toolchain, source commit, dirty-tree status and measurement limits. `tests.log`
retains the actual test results. Reports can contain machine-specific paths and
source text; they stay under ignored build output, not in Git.

The read-only [coverage workflow](../../.github/workflows/coverage.yml) runs the
same command on Linux for pushes and PRs to dev/main. It checks the downloaded
tool against a fixed SHA-256 before execution, uses a dated nightly, and retains
successful reports for five days. It does not send reports to an external coverage
service or change the existing eight required checks. No arbitrary percentage
threshold is enforced before a reliable cross-platform baseline exists.

## Scope and limits

- The suite runs unit tests, native CLI/TUI subprocess tests, the evaluation-test
  launcher and curated fuzz-adapter replay with the `fuzzing` feature enabled.
- Native subprocess profiles must be included. The runner rejects a report with
  an uncovered `src/main.rs` entry point or no instrumented branches.
- Evaluation tests inject the absolute profile destination through a test-only
  patch. Their production environment allowlist remains unchanged. Profiles must
  not appear as task edits or be ignored by the task grader.
- Dedicated test files and development fuzz-adapter files are excluded. Inline
  unit-test code inside ordinary source files remains in the denominator. These
  are source-file figures, not production-only coverage.
- Branch coverage uses experimental nightly instrumentation. Python, shell,
  dependencies, C/FFI and other operating systems are outside this measurement.
  Killed processes may not flush their final counters. No real provider is used.
- Line, region, function and branch percentages have different denominators. Do
  not substitute line coverage for branch coverage or claim the Best Practices
  `test_most` suggestion from these figures.

## First measured baseline

The initial local macOS ARM64 run on 2026-11-02 measured **84.93% lines**
and **66.19% branches**, with cargo-llvm-cov 0.9.1 and
rustc 1.101.0-nightly from nightly-2026-10-01. Its starting application source was
v0.3.17/main `068b603123f44fff5c564de1e081f361cd4d32c5`, plus the test-only
evaluation-profile routing correction. All 200 unit tests, the evaluation launcher
with 21 Python tests, 66 native integrations and curated replay passed.

API-key authentication measured 45.83% branches. The extension environment factory
measured 100% branches. The former identified a useful next target: rejected
credential stores and invalid key input. Neither number proves an implementation
is vulnerable or complete. The final local summary records the measurement after
adding those regression tests; Linux results must be read from actual workflow artifacts; these local macOS
figures do not establish Linux coverage.

The final local run measured **84.96% lines** and **66.31% branches** over the
same source-file denominators. API-key authentication rose from 45.83% to
58.33% branches. All 200 unit tests, the evaluation launcher with 21 Python checks,
68 native integrations and curated replay passed. The two new integrations cover
public, directory, symlink and malformed key stores, plus rejected blank key
input. They assert no provider call, no displayed secret and no credential-store
mutation. These are tests of existing behavior; runtime source is unchanged.
