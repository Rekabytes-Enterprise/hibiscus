# Current state

This is a working snapshot, **not a certificate that the product is bug-free**. See `MEMORY.md` for enduring constraints; use Git/`CHANGELOG.md` for history.

## Snapshot — 0.1.7 preparation, 2026-09-25 (UTC)

- Branch: `dev`; manifest version: `0.1.7`. This source batch builds on `aca6b10` and includes rendering, composer, activity/goal tracking, live queues, shared modals, documentation, tests, and the records cleanup. Use `git status`/`git log` for current commit and worktree provenance; the fingerprint below identifies tested source independently of commit metadata.
- `origin/dev` was fetched and matched `aca6b10` before preparation. The user requested a dev commit/push, not a release tag. Published releases and remote CI were not verified. A dev push must not be described as a deployed release or closure of the issues below.
- Version references and `CHANGELOG.md` were updated for 0.1.7. No real credential changes or provider runs were performed during preparation.
- Earlier user confirmations: WSL Alt+V image paste and Ctrl+Enter multiline input worked on earlier builds. They do not validate this newer worktree. The user's installed binary was not matched to this source snapshot.

## Latest automated evidence

Rerun on Linux after the 0.1.7 bump, against the source/test fingerprint below:

| Check | Result and scope |
| --- | --- |
| `cargo test --locked` | Pass: 80 unit + 72 integration tests. Includes mock Pi/SDK subprocesses and PTY scenarios; not a real-provider acceptance test. |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass; static checks only. |
| `cargo fmt --all -- --check` | Pass. |
| `node tests/approval.mjs` | Pass for the fake extension harness's policy/checklist cases, not arbitrary shell safety. |
| `node --check src/pi/approval.mjs` and `src/pi/logout.mjs` | Pass; syntax only. |
| `sh tests/install.sh` and `sh tests/bump-version.sh` | Pass with mocked downloads/sandbox fixtures; no real installation or release. |
| `git diff --check` | Pass; staging checks include newly added files before commit. |

Environment: Cargo 1.98.1, Node v24.21.0; `pi --version` reports 0.87.1. No macOS run or real terminal visual check was performed.

Source/test fingerprint: `78ab71dce31f1a468e01d6c10bc16d94b7b1cc3e672dc3191e1e9cbf703e4542`.
Computed as SHA-256 of sorted `path + NUL + file bytes + NUL` for `Cargo.toml`, `Cargo.lock`, `install.sh`, and files under `src/`, `tests/`, `scripts/`. Records/docs are excluded. **Any source/test change invalidates this snapshot's test claim until rerun.**

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

1. Stabilize the reported cursor/modal/queue cases before adding more UI behavior. Record the exact build/source, Pi version, terminal/platform and minimal reproduction; no credentials or private transcripts.
2. Reproduce queue ordering/identity risks in isolated fixtures, then add targeted regressions. Keep protocol behavior separate from presentation changes.
3. Run the relevant automated checks and retest the actual reported UX after rebuilding. Record **what was observed**, not just “fixed.” Keep unresolved items until evidence closes them.
4. After the requested dev push, check remote CI and supported platforms separately. Before publishing, verify the intended version-matching commit and release artifacts. No release tag is requested by this preparation.

Details live in `docs/terminal.md`, `docs/error-handling.md`, `docs/architecture.md`, and the named tests. Those documents describe intended/current implementation; they are not additional proof that a reported bug is resolved.
