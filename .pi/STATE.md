# Current state

## v0.3.5 release preparation, 2026-10-31

The owner requested releasing the new tag after approving the native runtime and CLI work. The latest published GitHub release was verified as v0.3.4. The release candidate is v0.3.5 on dev, based on c27070f, with all subsequent approved local changes included. The authorized destination is Rekabytes-Enterprise/hibiscus on GitHub: commit/push dev, annotate and push v0.3.5, and publish the GitHub Release through the repository workflow. npm publication is a separate event and is not part of that workflow. Existing untracked .agents remains untouched.

Version references in Cargo.toml, Cargo.lock, package.json, installation docs and installer fixtures are synchronized to 0.3.5. CHANGELOG.md and release notes cover the native migration, UI work and compatibility changes. Installation instructions use the installer from the v0.3.5 tag; the current main-branch installer still contains the older Pi bootstrap. No main-branch promotion is included in this release request.

Release publication and platform assets are pending remote CI/build verification. Do not infer published assets from this source version or a dev push. Verify the exact tag commit, workflow conclusion, four platform archives and SHA256SUMS before marking the release complete. A versioned npm wrapper is not implied by a GitHub release.

## Current implementation

Hibiscus owns its Rust agent loop, provider adapters, native file/shell/checklist tools, API-key and Codex OAuth stores, sessions, queues, compaction and terminal UI. It uses a bounded private in-process JSONL socket for presentation, with no Pi SDK, Node agent runtime or external credential/session import. Providers are configured for Responses, compatible Chat Completions and Anthropic Messages. Model IDs/capabilities are explicit config; unknown usage/context/pricing remains unknown.

Native tools run directly after validated arguments and authoritative provider completion. The approval system, slash command, preferences, grants and special dialogs are removed; old approval.json is ignored. Timeouts, cancellation, process-group cleanup, call/result IDs and unknown-outcome recovery remain. Tools are not sandboxed. No extension framework was added.

TUI remains the startup default with raspberry/aubergine styling and the solid slanted HIBI welcome. Active-model display follows credential availability; login selects the authenticated provider's configured model and logout preserves environment access. Login hints hide when credentials exist. /cli and /tui retain one runtime/session without resending prompts; CLI login/logout temporarily use native masked fullscreen dialogs.

CLI uses a dark background, compact metadata, grouped reads, green/red tool outcomes, actual shell output, measured duration/OS status, compact numbered edit previews and red/green diffs. /output latest/list/numbered inspection is local and read-only, with no provider call or tool rerun. Numbers refer to current active history; recent lists are bounded to fifty. Full saved bounded output and recorded edit diffs remain available across reopening that session; /new and compaction change active history.

CLI supports Enter-to-send, Alt+Enter/Ctrl+J newlines, arrows/Home/End/Backspace/Delete and atomic bracketed paste. Ordinary prompt whitespace is preserved. Invalid/oversized paste keeps the draft; CLI is capped at 64 KiB and TUI at 8 KiB. The editor repaints at most six draft rows plus a notice, using the existing terminal reader and locked unicode-width 0.2.2. NO_COLOR omits styling but interactive editing uses cursor/paste controls. TERM=dumb and nonterminal input retain the simple reader. Resize starts a fresh draft block on the next key. Paste mode is restored when leaving input and during cleanup.

## Verification

- v0.3.5: default parallel `TERM=dumb cargo test --locked` passes 155 unit and 39 native integration tests on this macOS host. The prior serial run also passed.
- Strict all-target Clippy, formatting, shell syntax, native installer, version-bump and offline npm package/npx tests pass. The npm tests publish nothing.
- Native fixtures verify provider protocols, completed-stream execution, file/shell tools, queues, cancellation, sessions, compaction, local credentials, masked login/logout, model/welcome state, mode switches, saved-output inspection and multiline input. OAuth fixtures cover loopback/PKCE/state, token exchange, serialized rotation and error preservation with synthetic credentials.
- Actual isolated CLI captures verify failed tests then edit/passing tests/Git diff; saved-output inspection adds no provider requests or tool executions. Multiline capture verifies zero requests before Enter and one exact three-line prompt afterward. Captures are locally rendered PTY bytes, not physical terminal-window or real-provider acceptance.

## Acceptance and measurement limits

Live provider/Codex account behavior, physical terminal key mappings/clipboard/fonts, Linux execution and all release targets require their corresponding acceptance evidence. Current remote release/CI results are pending. Native Codex browser login follows the legacy public-client protocol; device-code login, Claude browser login, OS-keychain storage and legacy session import are unavailable. Logout removes local credentials without revoking remote access. Read docs/native-runtime.md before upgrading old config/data.

Historical pre-plain-CLI measurements used one unchanged optimized native binary, fresh macOS PTYs and synthetic streaming. Median idle RSS was CLI3.84MiB/TUI4.20MiB; streaming sampled peak6.00/7.36MiB and CPU106/189ms. They exclude terminal/provider cost and do not measure this release's tool, input or long-session workload. No new performance claim follows from the UI changes. Pi-backed profiling docs are historical.
