# Contributing to Hibiscus

Thanks for helping improve Hibiscus. Start with the [architecture](docs/architecture.md), [terminal guide](docs/terminal.md), and [development guide](docs/development.md). Read `AGENTS.md` and the current `.pi/STATE.md` and `.pi/MEMORY.md` before changing code.

## Scope

Hibiscus owns the Rust agent loop, provider protocols, tools, authentication, sessions and terminal presentation. Keep external provider formats inside adapters. Validate tool arguments before execution and never run tools from an incomplete stream. Use native typed session/event data and preserve cancellation and request settlement. Do not restore a Pi SDK/runtime dependency. See [native migration](docs/native-runtime.md).

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

1. Keep the change focused and put it in the relevant `src/chat/`, `src/runtime/`, or `src/tui/` module. Update docs when usage, layout, or behavior changes.
2. Add a unit or integration regression test for a behavior change or bug when feasible. Local HTTP fixtures are suitable for provider behavior; use a controlling PTY for input/rendering/authentication behavior. Drain PTY output concurrently.
3. Run:

   ```sh
   cargo fmt --all -- --check
   cargo test --locked
   cargo clippy --locked --all-targets -- -D warnings
   ```

4. Pushes and pull requests to `dev` must pass `.github/workflows/ci.yml`: formatting, Clippy with warnings denied, installer checks, and Rust tests on Ubuntu and macOS. If behavior depends on a provider or a real terminal, perform and describe a manual smoke test as well. Clearly label anything tested only against mocks. Include reproduction steps and expected versus actual results in a pull request.
5. Reconcile `.pi/STATE.md` with current progress. Add to `.pi/MEMORY.md` only when a reusable decision or root cause is confirmed, updating an existing matching entry rather than duplicating it.

## Safety and privacy

Report suspected vulnerabilities through [GitHub's private reporting form](https://github.com/Rekabytes-Enterprise/hibiscus/security/advisories/new), following the [security policy](SECURITY.md). Keep vulnerability details out of public issues and pull requests. The policy explains supported versions, coordinated disclosure, and the limits of native tools, extensions, and checkpoint recovery.

Never commit API keys, OAuth data, `~/.hibiscus/auth.json`, private sessions, chat transcripts, or user-specific paths. Don't place real credentials in fixtures, logs, screenshots, or issue reports. Native tools execute directly after validation, including shell commands in non-interactive mode. Preserve bounded local framing, request-ID correlation and the run-settlement boundary.

Run the [pinned secret detector](docs/development/secret-scanning.md) before publishing changes. The required formatting/Clippy job also scans full Git history. Removing a credential from the latest file does not remove it from previous commits. Review false positives with a specific rule and fixture; do not add a blanket exclusion or bypass a real leak.

For `/login`, keep key entry masked and store credentials only in Hibiscus's private auth file. Do not log provider response bodies or keys. Authentication never launches another agent. Test with local HTTP fixtures and synthetic credentials. Preserve the existing session and recoverable draft after failures, and never replay a tool with an unknown outcome automatically.

## Required checks and review

Both `dev` and `main` require passing Linux/macOS tests, formatting/Clippy/installer checks, the RustSec dependency audit, and CodeQL analysis for Rust, JavaScript/TypeScript, Python and GitHub Actions. Update your PR branch when its target changes and resolve review threads before merging. The current single-maintainer policy has no independent-approval requirement or configured bypass actor. See [security checks and protected branches](docs/development/security-checks.md) for exact check names, scanner coverage, the future independent-review requirement and administrator recovery.
