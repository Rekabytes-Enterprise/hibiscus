# Current state

This is a working snapshot, **not a certificate that the product is bug-free**. See `MEMORY.md` for enduring constraints; use Git/`CHANGELOG.md` for history.

## Snapshot — shared image recovery checkpoint, 2026-09-25 (UTC)

- Branch: `dev`; manifest version: `0.1.7`; last pushed commit **`4e67a9e`**. The user requested a local commit for staged-image payload sharing (`Arc<Value>` plus serde `rc`), preserving JSONL image format and explicit `/restore`. Consult Git for the new checkpoint hash. **No push, version bump or tag** was requested. Remote CI for this newer source is not yet verified; earlier CI results below apply to older commits.
- Added optional Linux-only `scripts/profile-memory.py`, six helper tests in `tests/profile_memory.py`, and logical live/peak allocation-byte measurements in `benches/allocations.rs`. The helper launches a synthetic local Pi replacement solely for profiling; normal Hibiscus still launches real Pi. Credentials/Pi settings are isolated, clipboard helpers are mocked, and reports separate client and mock-process memory. No real models, sessions or clipboard contents were used.
- Completed **33 experiments** (11 scenarios × three fresh processes) against binary SHA-256 `fcf3ff78f7ab317f1d5fe0043091aea2ea208cec4794c3e9752fda3e44122472`, unchanged after verification. `docs/memory-profiling.md` records methodology, all results and caveats. Approximate client VmHWM medians: idle 3.06 MiB; 20 prose turns 9.00 MiB; four maximum images plus incoming echo 195.24 MiB at default limits; rejected queued images 123.24 MiB; ignored 2 MiB array 37.09 MiB versus text 7.15 MiB. Queue bursts hit their 8/32 MiB capacity budgets and explicitly disconnected.
- Shared-image evidence: cloning four maximum-size owned image values allocated **55,926,840 bytes** versus **32 bytes** for four shared handles. On the same synthetic Linux queue-rejection fixture (three runs), client observed `VmHWM` median fell from **123.24 MiB** to **69.64 MiB**; initial rejection remained ~69.74 MiB, and successful maximum-image send plus echo remained ~195.28 MiB. The latter path is still dominated by serialization/incoming echo. Baseline ignored-array parsing also cost **33,555,741 bytes** for ~2 MiB wire data; selective decoding is not implemented. Process RSS and these mock fixtures cannot establish real-provider behavior or prove absence of memory leaks. See `docs/memory-profiling.md`.
- Existing performance and bounded RPC-overload protection remain intact; see `docs/performance-review.md` and `docs/rpc-transport.md`. Still deferred: selective JSON decoding, the 40 ms shared event-loop wake-up redesign and background session scans. Kernel RSS/high-water counters and logical allocation instrumentation were used; call-stack profilers (`heaptrack`, `valgrind`, `perf`) were unavailable. No real-provider or physical-terminal validation was performed.
- Verified GitHub CI run [36140379851](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/36140379851) for `8616a00`: Ubuntu, macOS 14, and lint/installer jobs succeeded. This closes the reported macOS clipboard-fixture CI failure at that commit, not real desktop support. Verified newer CI run [36166061931](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/36166061931) for **`dc5212c`**: Ubuntu tests, macOS 14 tests, and formatting/Clippy/installer checks all succeeded. The records-only `dbba12f` run [36166398199](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/36166398199) also passed all three jobs. Those earlier runs predate the profiling tools; the profiling commit's CI result is recorded above. The optional Python profiler/helper checks and full measurement matrix were run locally, not by the current CI workflow. Published releases were not verified.
- The user reports completing the suggested manual checks with everything working. No platform/build-specific reproduction details were supplied, so this is positive user evidence, not closure of every outstanding verification item.
- Earlier user confirmations: WSL Alt+V image paste and Ctrl+Enter multiline input worked on earlier builds. They do not validate this newer worktree. The user's installed binary was not matched to this source snapshot.

## Latest automated evidence

The checks below were rerun on Linux after shared-image changes, against the source/test/benchmark fingerprint below. Full release-mode Rust testing was not performed; targeted release PTY suites and the allocation benchmark were run.

| Check | Result and scope |
| --- | --- |
| `cargo test --locked` | Local pass: 96 unit + 77 integration tests. Remote Ubuntu/macOS results listed above apply to older source, not this change. Includes mock Pi/SDK subprocesses and PTY scenarios; not a real-provider acceptance test. |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass; static checks only. |
| `cargo fmt --all -- --check` | Pass. |
| `cargo build --release --locked` | Pass; new binary SHA-256 `f0fb1cef5720bc9456633679e862516ca49fa2f271bbf08a3bbe3da5162ceb86` matches nine fresh shared-image follow-up experiments. |
| `python3 tests/profile_memory.py` | Pass: six tests for counters, framing, metrics and failed-report handling. |
| `python3 scripts/profile-memory.py --scenario {queue-reject,image-reject,image-send-max-default} --repeats 3 --output …` | Complete: nine fresh synthetic experiments; original 33-run baseline remains historical in the memory report. |
| `cargo bench --locked --bench allocations` | Pass; reports both owned-value and shared-handle cloning. No absolute timing CI gates. |
| `cargo test --locked --release --test image_paste --test steering_tty --test rpc_pressure --test performance_tty` | Pass: 16 targeted PTY tests for images, queues, pressure/cancellation and rendering. |
| Transport unit tests | Included in the full Rust run; seven scenarios covering bounds/duplex behavior. Prior opt-in counter output is documented in the RPC review. |
| `node tests/approval.mjs` | Pass for the fake extension harness's policy/checklist cases, not arbitrary shell safety. |
| `node --check src/pi/approval.mjs` and `src/pi/logout.mjs` | Pass; syntax only. |
| `sh tests/install.sh` and `sh tests/bump-version.sh` | Pass with mocked downloads/sandbox fixtures; no real installation or release. |
| `git diff --check` | Pass for tracked changes; untracked new files were reviewed separately. |

Environment: Cargo 1.98.1, Node v24.21.0, Python 3.12.3; `pi --version` reports 0.87.1. No local macOS run or real terminal visual check was performed here; the remote macOS CI result is recorded above.

Source/test/benchmark fingerprint: `2078221d2d805bdf7af2ef7586e70ffaf7c10a88622a47d584905521b0aea567`.
Computed as SHA-256 of sorted `path + NUL + file bytes + NUL` for `Cargo.toml`, `Cargo.lock`, `install.sh`, and files under `src/`, `tests/`, `scripts/`, `benches/`. Records/docs and generated Python bytecode (`__pycache__`, `*.pyc`) are excluded. **Any source/test change invalidates this snapshot's test claim until rerun.**

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
- **Not covered by this verification:** real macOS clipboard/OAuth/provider compatibility and published installer/update behavior. The verified remote CI runs are named above; do not extrapolate them to future source changes.

## Next actions, in order

1. Shared images are implemented locally and pass synthetic before/after checks. Retest real provider/clipboard flows and macOS CI for this source before release. Next performance candidate: avoid materializing unused incoming JSON; keep strict framing, validation, IDs, errors, approvals, settlement and event ordering unchanged.
2. Stabilize the reported cursor/modal/queue cases before adding more UI behavior. Record the exact build/source, Pi version, terminal/platform and minimal reproduction; no credentials or private transcripts.
3. Reproduce queue ordering/identity risks in isolated fixtures, then add targeted regressions. Keep protocol behavior separate from presentation changes.
4. Run the relevant automated checks and retest the actual reported UX after rebuilding. Record **what was observed**, not just “fixed.” Keep unresolved items until evidence closes them.
5. Before publishing, verify the intended version-matching commit, supported-platform CI and release artifacts. No release tag/version bump is part of this image-sharing task.

Details live in `docs/terminal.md`, `docs/error-handling.md`, `docs/architecture.md`, and the named tests. Those documents describe intended/current implementation; they are not additional proof that a reported bug is resolved.
