# Development

## Set up

Install Rust/Cargo, Pi, and a C linker. Inline Codex sign-in also needs Node.js 22.19+ to load the installed Pi SDK; without it Hibiscus falls back to Pi's TUI. On Ubuntu/WSL, install the linker with `sudo apt update && sudo apt install -y build-essential`. Pi must be available as `pi` on `PATH`, or `HIBISCUS_PI` must point to its executable. Hibiscus never installs Pi silently. For a nonstandard Pi install, `HIBISCUS_AUTH_SDK` can point to that trusted Pi package's `dist/index.js` (and `HIBISCUS_NODE` can override the Node executable). The SDK path is executable code; never point it at an untrusted file.

From the repository root:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo install --path .
```

Reinstall after modifying the source before testing the `hibiscus` command on your PATH. `cargo run --` uses the working tree without installing it. See [Installation and releases](installation.md) for the checksum-verifying prebuilt installer and four-target release workflow.

## Continuous integration

`.github/workflows/ci.yml` runs on pushes and pull requests to `dev` only, with manual dispatch available. Direct pushes and pull requests targeting `main` do not trigger this CI workflow. The lint job checks `cargo fmt`, runs Clippy with warnings denied, validates both installer scripts, and executes the mocked installer test. Rust tests run independently on Ubuntu and macOS with parent `TERM=dumb`; full-screen PTY fixtures explicitly set child `TERM=xterm-256color` and an 80×24 window instead of inheriting the runner's terminal capabilities. Jobs use read-only repository permissions and redundant runs on the same ref are cancelled.

Tagged releases use the separate `.github/workflows/release.yml` gate, which repeats the quality checks before building and publishing all supported target archives. A green CI run does not replace real Pi/provider or terminal smoke testing.

## Test boundaries

- Module unit tests cover framing, prompt settlement, picker bounds, command filtering, Markdown, diffs, and terminal render state.
- `tests/chat.rs` and `tests/in_chat_sessions.rs` run against mock Pi executables with piped input/output.
- `tests/tty.rs`, `tests/models_tty.rs`, `tests/dialog_tty.rs`, `tests/auth_handoff.rs`, and `tests/codex_auth.rs` use a controlling PTY to exercise raw input, UI output, interrupts, pickers, and both authentication paths. Codex tests use a fake SDK module with no network or real credentials. `completed_login_reconnects_without_waiting_for_browser_connection_to_close` keeps a local callback-like socket open after SDK completion and verifies that login still returns promptly. A PTY test must **drain its output concurrently**; otherwise full-screen redraws can fill the PTY buffer and hang the child.
- The mock tests do not verify provider authentication, network APIs, or the appearance in every terminal. Do not claim real-Pi behavior based only on mocks.

When changing JSONL handling, preserve LF-only framing, keep stderr separate, correlate responses by command ID, and wait for `agent_settled` rather than treating prompt acceptance or `agent_end` as completion. Keep regression coverage for aborts and subsequent prompts. When changing terminal input, test Esc against mouse/modified-key sequences and restore raw mode, mouse reporting, and alternate screen on all exit paths.

## Manual smoke checks

With a configured Pi model and an interactive terminal, test:

1. Send a short prompt, then another. Use Esc during a slow response; chat should remain usable.
2. Restart with `hibiscus --continue`, and switch sessions with `/sessions`. Check only the selected session's recent five user turns and assistant text.
3. Open `/models`, navigate a list longer than five items, cancel with Esc, and switch models. The header should reflect Pi's active model.
4. Type `/m`, use Tab and Enter in the live command suggestions, and check that only Hibiscus commands appear.
5. Check the flower heartbeat during a silent wait and inspect a successful `edit` preview against `git diff`. Confirm that errors show failure, not an invented diff.
6. With no saved chat and no Codex model selected, open `/login`, choose Codex, and try both browser and device-code flows with informed consent (including the WSL manual redirect fallback and cancellation). Leave the browser's success tab open after callback and verify Hibiscus returns to chat anyway, then verify Pi can answer through Hibiscus after the RPC child reconnects. Esc should cancel while login is waiting; Ctrl+C is a raw-mode key, not necessarily a shell signal. Do not record URLs, codes, or tokens. Also test the **other provider** choice and `/logout`: Pi's TUI should open with `--no-session` for an empty chat, with no concurrent RPC child. After `/quit` in Pi, Hibiscus should reconnect to the previous session. Real-provider credential refresh still needs verification.
7. Check `NO_COLOR=1`, a narrow terminal, and `printf 'hello\n' | hibiscus` for safe fallback/plain output.

Never paste credentials or private session transcripts into test fixtures or documentation. For session and resource locations, see [Architecture](architecture.md).

## Project records

Read `AGENTS.md`, `.pi/STATE.md`, and `.pi/MEMORY.md` before planning changes. Update `STATE.md` with current work and verification blockers; update `MEMORY.md` only for confirmed, reusable decisions or pitfalls. Revise an existing entry when a new finding has the same root cause instead of adding a duplicate. See [CONTRIBUTING.md](../CONTRIBUTING.md) for the contribution checklist.
