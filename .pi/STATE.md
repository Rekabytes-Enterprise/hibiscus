# Current state

This is a working snapshot, **not a certificate that the product is bug-free**. See `MEMORY.md` for enduring constraints; use Git/`CHANGELOG.md` for history.

## Snapshot — performance checkpoint, 2026-09-25 (UTC)

- Branch: `dev`; manifest version: `0.1.7`; last pushed commit: `8616a00`. Pending local changes add the standard MIT license (`LICENSE`, `Cargo.toml` metadata, README link), owner attribution and a fork → feature branch from upstream `dev` → PR to `dev` walkthrough in `CONTRIBUTING.md`, plus a clearer `docs/architecture.md` overview/codebase map/runtime flow/change guide. The user identifies Hibiscus as a Reka Bytes project. Public `https://reka-bytes.my/about` identifies the trading name as Reka Bytes and the Malaysian business name as Rekabytes Enterprise, registration 202503277241 (IP0614333-M). These pages do not independently establish chain of title to Hibiscus. Standard MIT allows commercial use and resale. The user requested a local checkpoint commit before continuing RPC backpressure work, not a push.
- Implemented locally: linear slice-based Markdown wrapping and failed-link search handling, direct character cells, cached completed transcript blocks/scroll row counts, bounded printable-input batching/chunked terminal reads, cached workspace text, bounded/expiring session metadata, borrowed image command serialization, moved response data, reusable RPC line buffers, and move-only clipboard base64 insertion. `docs/performance-review.md` records before/after evidence and remaining work. Prior licensing/contribution changes remain intact; the checkpoint has no version bump or release tag. Consult Git for its commit identifier.
- Added `benches/{formatter,allocations,session_list}.rs` and `tests/performance_tty.rs`, plus unit regressions. On the original local synthetic PTY harness, a 521-byte paste into a 256 KiB transcript changed from 2.3 s to 0.31 ms; the committed PTY harness reports about 10 ms including driver polling. These timings are host-specific; no real provider/session data or physical-terminal presentation was tested.
- Deliberately deferred: 40 ms RPC wait/event-loop redesign, inbound queue backpressure/partial JSON decoding, background session scans, and remaining rejected-draft copies. No peak-RSS/backlog profile or duplex stress proof was obtained; blindly blocking the RPC reader risks cancellation/pipe deadlocks. First session listing still scans files synchronously. No sampling profiler was used (`perf` was unavailable).
- The preceding macOS clipboard-fixture correction is pushed, but a macOS CI rerun has not been checked; no claim of macOS or real-provider verification follows from the Linux checks below.
- The supplied macOS 0.1.7 CI log passes 80 unit tests and the preceding integration suites, then fails seven of eight steering cases while waiting for an image attachment. `tests/steering_tty.rs` mocked only `wl-paste`, whereas the compiled macOS backend in `src/tui/clipboard.rs` selects `osascript` first and ignores Wayland environment settings. The tests fell through to the runner clipboard instead of their image fixture; the no-image result precedes steering submission.
- The pushed fixture now shadows all four platform reader names and logs helper invocations. A regression checks each fake reader's bytes/type-list response; steering cases assert the host's expected mock reader was used. **A macOS CI rerun is still required**. This fixture correction does not establish real macOS clipboard or provider support. Published releases/remaining CI jobs were not verified.
- The user reports that the current work is working and authorized the next RPC-backpressure phase. No platform/build-specific reproduction details were supplied, so this does not close every outstanding verification item.
- Earlier user confirmations: WSL Alt+V image paste and Ctrl+Enter multiline input worked on earlier builds. They do not validate this newer worktree. The user's installed binary was not matched to this source snapshot.

## Latest automated evidence

The full checks below were rerun on Linux after the performance changes, against the source/test/benchmark fingerprint below. Full release-mode Rust testing was not performed; the targeted release PTY regression and benchmarks were run.

| Check | Result and scope |
| --- | --- |
| `cargo test --locked` | Pass: 88 unit + 74 integration tests. Includes mock Pi/SDK subprocesses and PTY scenarios; not a real-provider acceptance test. |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass; static checks only. |
| `cargo fmt --all -- --check` | Pass. |
| `cargo build --release --locked` | Pass during implementation; the final release binary was also built by the targeted release PTY test. |
| `cargo bench --locked --bench formatter --bench allocations --bench session_list` | Pass; reports timings/allocation calls, no absolute timing CI gates. |
| `cargo test --locked --release --test performance_tty -- --nocapture` | Pass; exact pasted text and batched-frame regression. |
| `node tests/approval.mjs` | Pass for the fake extension harness's policy/checklist cases, not arbitrary shell safety. |
| `node --check src/pi/approval.mjs` and `src/pi/logout.mjs` | Pass; syntax only. |
| `sh tests/install.sh` and `sh tests/bump-version.sh` | Pass with mocked downloads/sandbox fixtures; no real installation or release. |
| `git diff --check` | Pass for tracked changes; untracked new files were reviewed separately. |

Environment: Cargo 1.98.1, Node v24.21.0; `pi --version` reports 0.87.1. No macOS rerun of these changes or real terminal visual check was performed here.

Source/test/benchmark fingerprint: `01e5fe725e6a020f00c0abfb60d499478c11529898725e6e899fbcb9276bcb44`.
Computed as SHA-256 of sorted `path + NUL + file bytes + NUL` for `Cargo.toml`, `Cargo.lock`, `install.sh`, and files under `src/`, `tests/`, `scripts/`, `benches/`. Records/docs are excluded. **Any source/test change invalidates this snapshot's test claim until rerun.**

## User-reported issues: candidates implemented, real-use closure pending

Do not mark these resolved merely because the automated suite passes.

| Reported problem | Current code / automated evidence | Still needed |
| --- | --- | --- |
| Provider errors close interactive chat | Typed request/transport errors and run-outcome tracking are present; `tests/error_recovery.rs` covers synthetic rate-limit, auth, retry, crash and reconnect scenarios. | Verify a real reported failure on the rebuilt version without deliberately causing billable operations or changing credentials. Confirm chat and recovery commands remain usable. |
| Cursor flickers while a reply streams | `tui/paint.rs` caches rows and groups writes with synchronized-update markers; `screen.rs` batches text flushes and requests a steady cursor. `tests/rendering_tty.rs` and `tests/working_cursor_tty.rs` check bytes, batching and cursor positioning. | Retest the reported streaming case in Windows Terminal/WSL. Tests cannot prove that the terminal presents frames without flicker. Check typing, resize, sync-disabled fallback and handoff too. |
| Approval prompt overwrites composer/footer | `tui/modal.rs` supplies common panel rendering; full-screen Pi dialogs route through Screen instead of raw prompts. Covered by `tests/modal_tty.rs`, `tests/approval_tty.rs` and auth fixtures. | Real Pi approval/modal interaction after rebuilding; preserve draft and keyboard focus across cancellation, timeout and long details. |
| Steering appears in chat before delivery | Current UI keeps a pending panel and uses user `message_start` for transcript insertion. `tests/steering_tty.rs` covers selected mock event orders. | Real steering/follow-up delivery, identical messages, images, rejection, cancellation and disconnect. No claim of complete parity with Pi TUI. |
| Explore looks complete while Pi is still working | `tui/activity.rs` keeps an exploration state between read calls and animates a glyph. Edits/writes or freezing the activity block end it. | Check real event cadence and intermediate assistant commentary. A completed read is not completion of the whole task. |

Also pending: inline `/logout` against an intentionally selected account, the current shared-modal Codex login flow, and `/new` clearing the view while retaining saved sessions. Existing tests use fake credentials/providers; **do not log out real accounts or induce billable failures just to validate records**.

## Code-observed limits and review risks

- **Unicode layout:** composer/row helpers count Unicode scalars, not grapheme clusters or terminal cell widths (`tui/screen.rs`). UTF-8-safe edits do not establish correct CJK/emoji/combining-character cursor alignment.
- **Approval guard:** regex-based `pi/approval.mjs` can over-prompt (including redirections) and miss indirect destructive effects. `read`/`write`/`edit` are not sandboxed. “Always Allow” is an exact command/cwd grant, not a guarantee that repeated execution has unchanged effects.
- **Modal review wording:** `Modal::rows` sets `reviewed` when the last detail page fits/is displayed. It does not independently track every page viewed or prove that the user read the full command. Previous blanket “full details reviewed” claims were too strong.
- **Goal percentage:** the extension counts steps marked complete by the model. It does not independently verify that the work was performed. No explicit checklist means no percentage; tool-call counts are not task completion.
- **Activity boundaries:** `Screen::freeze_timeline` runs on assistant text as well as settlement. Summaries are per activity block, not necessarily one global summary for an entire long task (`pi/rpc.rs`, `tui/activity.rs`).
- **Queue identity:** pending UI matches text/order; initial prompt echo suppression compares text (`Screen::delivered_user`). This is not a stable per-message identifier. Expanded/rewritten prompts and untested duplicate/ack orderings remain risks.
- **Hypothesis to reproduce:** `rpc::exchange` resets `settled` on accepted queued responses. Check an acknowledgement arriving *after* the final settlement, with no later event. Current mock cases do not establish correct behavior for every ordering; do not record a root cause/fix before reproducing it.
- **Not covered by this verification:** real macOS clipboard/OAuth/provider compatibility, published installer/update behavior, and latest remote CI. Old release-check results were removed rather than treated as current evidence.

## Next actions, in order

1. Rerun macOS CI with the corrected fixture; confirm all steering cases and the subsequent suites finish. Do not label the macOS failure resolved until that result is available.
2. Stabilize the reported cursor/modal/queue cases before adding more UI behavior. Record the exact build/source, Pi version, terminal/platform and minimal reproduction; no credentials or private transcripts.
3. Reproduce queue ordering/identity risks in isolated fixtures, then add targeted regressions. Keep protocol behavior separate from presentation changes.
4. Run the relevant automated checks and retest the actual reported UX after rebuilding. Record **what was observed**, not just “fixed.” Keep unresolved items until evidence closes them.
5. Before publishing, verify the intended version-matching commit, supported-platform CI and release artifacts. No release tag/version bump is part of the pending documentation/licensing work or performance implementation.

Details live in `docs/terminal.md`, `docs/error-handling.md`, `docs/architecture.md`, and the named tests. Those documents describe intended/current implementation; they are not additional proof that a reported bug is resolved.
