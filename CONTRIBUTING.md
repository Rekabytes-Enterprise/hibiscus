# Contributing to Hibiscus

Thanks for helping improve Hibiscus. Start with the [architecture](docs/architecture.md), [terminal guide](docs/terminal.md), and [development guide](docs/development.md). Read `AGENTS.md` and the current `.pi/STATE.md` and `.pi/MEMORY.md` before changing code.

## Scope

Hibiscus owns the Rust CLI, terminal presentation, and Pi RPC integration. Pi owns agent behavior, model metadata, tool execution, authentication, and session persistence. Prefer calling Pi through RPC to reimplementing any of these in Hibiscus. Do not add a second credential store or silently install Pi.

## Fork, branch, and open a pull request

1. On GitHub, **fork** [Rekabytes-Enterprise/hibiscus](https://github.com/Rekabytes-Enterprise/hibiscus) to your account.
2. Clone **your fork**, add the original repository as `upstream`, and create **your own branch from the latest `dev`** (replace `your-username` and `your-feature`):

   ```sh
   git clone https://github.com/your-username/hibiscus.git
   cd hibiscus
   git remote add upstream https://github.com/Rekabytes-Enterprise/hibiscus.git
   git fetch upstream dev
   git switch -c your-feature upstream/dev
   ```

3. Make a focused change and run the checks below. Commit and push **your branch to your fork**, not to the project's `dev` branch:

   ```sh
   git add .
   git commit -m "Describe your change"
   git push -u origin your-feature
   ```

   Before `git add .`, check `git status` and exclude credentials, private sessions, and generated files.
4. On GitHub, open a pull request with **base repository** `Rekabytes-Enterprise/hibiscus`, **base branch** `dev`, and **head branch** `your-username:your-feature`. **Do not target `main`**; CI runs for PRs targeting `dev`. Describe the change, reproduction steps, expected/actual results, and what you tested (including any manual checks or blockers). Address review feedback on the same branch; the PR updates automatically when you push.

## Making a change

1. Keep the change focused and put it in the relevant `src/chat/`, `src/pi/`, or `src/tui/` module. Update docs when usage, layout, or behavior changes.
2. Add a unit or integration regression test for a behavior change or bug when feasible. Mock Pi subprocesses are suitable for protocol behavior; use a controlling PTY for input/rendering/handoff behavior. Drain PTY output concurrently.
3. Run:

   ```sh
   cargo fmt --all -- --check
   cargo test --locked
   cargo clippy --locked --all-targets -- -D warnings
   ```

4. Pushes and pull requests to `dev` must pass `.github/workflows/ci.yml`: formatting, Clippy with warnings denied, installer checks, and Rust tests on Ubuntu and macOS. If behavior depends on Pi or a real terminal, perform and describe a manual smoke test as well. Clearly label anything tested only against mocks. Include reproduction steps and expected versus actual results in a pull request.
5. Reconcile `.pi/STATE.md` with current progress. Add to `.pi/MEMORY.md` only when a reusable decision or root cause is confirmed, updating an existing matching entry rather than duplicating it.

## Safety and privacy

Never commit API keys, OAuth data, `~/.pi/agent/auth.json`, private Pi sessions, chat transcripts, or user-specific paths. Don't place real credentials in fixtures, logs, screenshots, or issue reports. Extension dialogs must never auto-approve in non-interactive mode. Preserve LF-delimited RPC framing, command-ID correlation, separate stderr diagnostics, and the `agent_settled` completion boundary.

For Codex `/login`, use Pi's SDK for OAuth/storage; never implement a second token store, log authorization codes, or echo manually pasted redirects. Treat the helper's explicit completion event (after Pi finishes credential synchronization) as authoritative; do not wait for its natural exit because the OAuth callback server can leave browser sockets open. Terminate and reap the dedicated helper after protocol completion, without changing the reported success outcome. Reconnect the idle RPC child after a successful sign-in without opening the same saved session in two Pi children. The Pi TUI fallback (`/logout`, other providers, or unavailable SDK) must first close the idle RPC child, stop the raw terminal reader, and restore terminal modes before handoff, then restore the previous session in Hibiscus afterward. Auth must work before the first saved chat message; do not tie `/login` to the active model. If a real-Pi check is unavailable, record that limitation instead of claiming it worked.
