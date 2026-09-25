# Current state

This is a working snapshot, **not a certificate that the product is bug-free**. See `MEMORY.md` for enduring constraints; use Git/`CHANGELOG.md` for history.

## Snapshot — RPC dev push preparation, 2026-09-25 (UTC)

- Branch: `dev`; manifest version: `0.1.7`. Performance/license/contribution/architecture checkpoint: **`7c0f5e8`**. The user authorized proceeding with the proposed RPC commit/dev push and CI verification. Source fingerprint was rechecked against the tested snapshot below; use Git for the RPC commit identifier. No version bump or release tag is part of this task.
- The checkpoint contains Markdown/layout/input/session-cache optimizations, lower-copy image/RPC serialization, benchmarks, MIT attribution and the contribution guide targeting `dev`. Those changes remain intact. `docs/performance-review.md` records their before/after evidence.
- Added `benches/{formatter,allocations,session_list}.rs` and `tests/performance_tty.rs`, plus unit regressions. On the original local synthetic PTY harness, a 521-byte paste into a 256 KiB transcript changed from 2.3 s to 0.31 ms; the committed PTY harness reports about 10 ms including driver polling. These timings are host-specific; no real provider/session data or physical-terminal presentation was tested.
- New `src/pi/transport.rs`: byte/count-bounded raw-record inbox, bounded record assembly, lazy consumer-side JSON parsing, explicit overload invalidation, nonblocking/deadline-controlled writes, and opt-in high-water counters. Defaults are 64 MiB allocated queued capacity and 4,096 records; limits are configurable. This is **fail-fast overload protection**, not transparent flow control or a total RSS cap. It intentionally avoids blocking the stdout reader on a full queue.
- Initial/queued sends service keys/ticks; Esc on a partial record disconnects with uncertain delivery rather than corrupting JSONL with an appended abort. Failed sends and the newest unacknowledged queued draft are reviewable without automatic replay. Shutdown waits are bounded. `docs/rpc-transport.md` documents configuration, trade-offs and recovery.
- New evidence: seven transport tests (including slow FIFO consumption, byte/count/capacity caps, no-LF limits, blocked-write deadlines/callback cancellation and a duplex stdout-flood/stdin-block scenario) plus three PTY pressure tests (modal fail-closed approval/reconnect, blocked initial image send, blocked queued image send). Counter checks stayed within their test budgets; the duplex fixture peaked at 264 queued bytes/eight records before explicit failure. These are synthetic scenarios, not real-provider load or total-RSS measurements.
- Still deferred: the 40 ms shared event-loop wake-up redesign, partial JSON decoding, background session scans and remaining rejected-draft copies. First session listing still scans files synchronously. No sampling/peak-RSS profiler or real macOS/provider verification was performed.
- Verified GitHub CI run [36140379851](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/36140379851) for `8616a00`: Ubuntu, macOS 14, and lint/installer jobs succeeded. This closes the reported macOS clipboard-fixture CI failure at that commit, not real desktop support. **CI for the newer performance/RPC changes is pending this push.** Published releases were not verified.
- The user reports completing the suggested manual checks with everything working. No platform/build-specific reproduction details were supplied, so this is positive user evidence, not closure of every outstanding verification item.
- Earlier user confirmations: WSL Alt+V image paste and Ctrl+Enter multiline input worked on earlier builds. They do not validate this newer worktree. The user's installed binary was not matched to this source snapshot.

## Latest automated evidence

The full checks below were rerun on Linux after the RPC changes, against the source/test/benchmark fingerprint below. Full release-mode Rust testing was not performed; both targeted release PTY suites and the benchmarks were run.

| Check | Result and scope |
| --- | --- |
| `cargo test --locked` | Pass: 95 unit + 77 integration tests. Includes mock Pi/SDK subprocesses and PTY scenarios; not a real-provider acceptance test. |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass; static checks only. |
| `cargo fmt --all -- --check` | Pass. |
| `cargo build --release --locked` | Pass during implementation; the final release binary was also built by the targeted release PTY test. |
| `cargo bench --locked --bench formatter --bench allocations --bench session_list` | Pass; reports timings/allocation calls, no absolute timing CI gates. |
| `cargo test --locked --release --test rpc_pressure --test performance_tty` | Pass: four tests for pressure/cancellation and exact pasted text/batched frames. |
| `HIBISCUS_RPC_METRICS=1 cargo test --locked --bin hibiscus pi::transport::tests -- --nocapture` | Pass: seven synthetic transport tests; counters only, not an RSS profile. |
| `node tests/approval.mjs` | Pass for the fake extension harness's policy/checklist cases, not arbitrary shell safety. |
| `node --check src/pi/approval.mjs` and `src/pi/logout.mjs` | Pass; syntax only. |
| `sh tests/install.sh` and `sh tests/bump-version.sh` | Pass with mocked downloads/sandbox fixtures; no real installation or release. |
| `git diff --check` | Pass for tracked changes; untracked new files were reviewed separately. |

Environment: Cargo 1.98.1, Node v24.21.0; `pi --version` reports 0.87.1. No macOS rerun of these changes or real terminal visual check was performed here.

Source/test/benchmark fingerprint: `4afeadac802408402bd6fe993476636f79b76a043731ae351ee735a75f335340`.
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

1. Verify Linux/macOS CI for the RPC dev push. The older clipboard-fixture CI failure is verified resolved at `8616a00`; keep that distinct from validation of newer changes. Test real workloads/default limits before any release.
2. Stabilize the reported cursor/modal/queue cases before adding more UI behavior. Record the exact build/source, Pi version, terminal/platform and minimal reproduction; no credentials or private transcripts.
3. Reproduce queue ordering/identity risks in isolated fixtures, then add targeted regressions. Keep protocol behavior separate from presentation changes.
4. Run the relevant automated checks and retest the actual reported UX after rebuilding. Record **what was observed**, not just “fixed.” Keep unresolved items until evidence closes them.
5. Before publishing, verify the intended version-matching commit, supported-platform CI and release artifacts. No release tag/version bump is part of the checkpoint or current RPC work.

Details live in `docs/terminal.md`, `docs/error-handling.md`, `docs/architecture.md`, and the named tests. Those documents describe intended/current implementation; they are not additional proof that a reported bug is resolved.
