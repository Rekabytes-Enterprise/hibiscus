# Current state

This is a working snapshot, **not a certificate that the product is bug-free**. See `MEMORY.md` for enduring constraints; use Git/`CHANGELOG.md` for history.

## Snapshot — late queue acknowledgement dev push verified, 2026-09-25 (UTC)

- Branch: `dev`; manifest version: `0.1.7`. Pushed session scan checkpoint **`890485d`** and late-ack RPC fix **`3a06df2`** to `origin/dev`. GitHub CI [36181935123](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/36181935123) for `3a06df2` passed macOS 14 tests, Ubuntu tests and formatting/Clippy/installer checks. This records-only update leaves the tested source fingerprint unchanged. No version bump/tag. The user reported their session-scan manual check worked without build/platform details. The requested binary digest remains absent from current tracked records; local profiling reports stay outside Git.
- Added optional Linux-only `scripts/profile-memory.py`, six helper tests in `tests/profile_memory.py`, and logical live/peak allocation-byte measurements in `benches/allocations.rs`. The helper launches a synthetic local Pi replacement solely for profiling; normal Hibiscus still launches real Pi. Credentials/Pi settings are isolated, clipboard helpers are mocked, and reports separate client and mock-process memory. No real models, sessions or clipboard contents were used.
- Completed **33 experiments** (11 scenarios × three fresh processes) against a locally fingerprinted release binary, unchanged during the experiments. `docs/memory-profiling.md` records methodology, all results and caveats. Approximate client VmHWM medians: idle 3.06 MiB; 20 prose turns 9.00 MiB; four maximum images plus incoming echo 195.24 MiB at default limits; rejected queued images 123.24 MiB; ignored 2 MiB array 37.09 MiB versus text 7.15 MiB. Queue bursts hit their 8/32 MiB capacity budgets and explicitly disconnected.
- Shared-image evidence: cloning four maximum-size owned image values allocated **55,926,840 bytes** versus **32 bytes** for four shared handles. On the same synthetic Linux queue-rejection fixture (three runs), client observed `VmHWM` median fell from **123.24 MiB** to **69.64 MiB**; initial rejection remained ~69.74 MiB, and successful maximum-image send plus echo remained ~195.28 MiB. The latter path is still dominated by serialization/incoming echo. Baseline ignored-array parsing also cost **33,555,741 bytes** for ~2 MiB wire data. The narrow decoder follow-up reduced synthetic client observed `VmHWM` median for that ignored array from **37.09 MiB** to **7.18 MiB** (three runs); text control remained ~7.22 MiB. The new incoming user-image echo follow-up reduced synthetic client observed `VmHWM` median for a successful maximum-image send from **195.28 MiB** to **141.83 MiB** (three fresh runs). All runs completed without inbox failure and retained the 64 MiB raw queue peak. Broader selective decoding of authoritative message ends/history remains unimplemented. Process RSS and these mock fixtures cannot establish real-provider behavior or prove absence of memory leaks. See `docs/memory-profiling.md`.
- The shared input/RPC wake follow-up is measured in `docs/wake-profiling.md`: during a silent synthetic active run, 60 key-to-output observations changed from **21.02 ms median / 37.09 ms p95** to **0.26 ms median / 0.49 ms p95** on this Linux/WSL host. A second post-change run gave 0.25/0.46 ms. This is PTY byte timing, not visible-cursor or real-provider performance. Existing bounded RPC-overload protection remains intact. Full-screen session scans now run on a worker, but background prefetch and startup/line-mode async listing are not implemented. Still deferred: broader selective decoding (including authoritative message ends/history) and physical-terminal acceptance checks. Kernel RSS/high-water counters and logical allocation instrumentation were used; call-stack profilers (`heaptrack`, `valgrind`, `perf`) were unavailable. No real-provider or physical-terminal validation was performed.
- Verified GitHub CI run [36140379851](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/36140379851) for `8616a00`: Ubuntu, macOS 14, and lint/installer jobs succeeded. This closes the reported macOS clipboard-fixture CI failure at that commit, not real desktop support. Verified newer CI run [36166061931](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/36166061931) for **`dc5212c`**: Ubuntu tests, macOS 14 tests, and formatting/Clippy/installer checks all succeeded. The records-only `dbba12f` run [36166398199](https://github.com/Rekabytes-Enterprise/hibiscus/actions/runs/36166398199) also passed all three jobs. Those earlier runs predate the profiling tools; the profiling commit's CI result is recorded above. The optional Python profiler/helper checks and full measurement matrix were run locally, not by the current CI workflow. Published releases were not verified.
- The user reports completing the suggested manual checks with everything working. No platform/build-specific reproduction details were supplied, so this is positive user evidence, not closure of every outstanding verification item.
- Earlier user confirmations: WSL Alt+V image paste and Ctrl+Enter multiline input worked on earlier builds. They do not validate this newer worktree. The user's installed binary was not matched to this source snapshot.

## Latest automated evidence

The checks below were rerun on Linux after the late-ack regression/fix, against the source/test/benchmark fingerprint below. Full release-mode Rust testing was not performed; targeted release PTY suites were run.

| Check | Result and scope |
| --- | --- |
| `cargo test --locked` | Local pass: 103 unit + 80 integration tests; remote Ubuntu/macOS jobs passed on `3a06df2`. These are mock/PTY scenarios, not real-provider acceptance tests. |
| `cargo clippy --locked --all-targets -- -D warnings` | Pass; static checks only. |
| `cargo fmt --all -- --check` | Pass. |
| `cargo build --release --locked` | Pass; the local wake profiler ran against the release build. No binary digest is stored in this record; older digests already in Git history are not rewritten by this task. |
| `python3 tests/profile_memory.py` | Pass: six tests for counters, framing, metrics and failed-report handling. |
| `python3 scripts/profile-wake.py --samples 60 --output …` | Pass: synthetic before/after quiet-run latency comparison; results and limits in `docs/wake-profiling.md`. Earlier memory profiling remains historical. |
| `python3 tests/profile_wake.py` | Pass: two synthetic/probe helper tests; no real Pi calls. |
| `cargo bench --locked --bench session_list` | Pass: on this host, 200 files cold scan ~94 ms, cached repeat ~0.26 ms. The worker does not speed up cold I/O; the UI can show loading/cancel while it runs. |
| `cargo test --locked --release --test queue_order_tty --test steering_tty --test rpc_pressure --test session_scan_tty --test error_recovery --test rendering_tty --test approval_tty` | Pass: 27 targeted release PTY/integration tests. Both queue-order scenarios also passed five repeated release runs. |
| Transport unit tests | Included in the full Rust run; seven scenarios covering bounds/duplex behavior. Prior opt-in counter output is documented in the RPC review. |
| `node tests/approval.mjs` | Pass for the fake extension harness's policy/checklist cases, not arbitrary shell safety. |
| `node --check src/pi/approval.mjs` and `src/pi/logout.mjs` | Pass; syntax only. |
| `sh tests/install.sh` and `sh tests/bump-version.sh` | Pass with mocked downloads/sandbox fixtures; no real installation or release. |
| `git diff --check` | Pass for tracked changes; untracked new files were reviewed separately. |

Environment: Cargo 1.98.1, Node v24.21.0, Python 3.12.3; `pi --version` reports 0.87.1. No local macOS run or real terminal visual check was performed here; remote macOS CI for this source passed as recorded above.

Source/test/benchmark fingerprint: `12f3e41c00659b7983ef8a0b7afeb3d66f14db822cdbcd3ed2fa7b4a5f0de655`.
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
- **Reproduced late-ack hang and targeted fix:** `tests/queue_order_tty.rs` initially timed out when Pi's final queue drain/`agent_settled` preceded a queued success response with no later event. `rpc::exchange` both reset `settled` on acceptance and skipped its common completion check via `continue`. Queue acceptance no longer resets settlement; the response branch checks correlated acceptance, settlement, pending requests and Pi queue counts before returning. A second fixture confirms nonempty queues still wait for a later empty update. An earlier synthetic steering fixture now emits `agent_start` for a new run after early settlement, rather than relying on an acknowledgement to invent that boundary. These fixtures do not prove every real Pi ordering or solve text-based queue identity.
- **Not covered by this verification:** real macOS clipboard/OAuth/provider compatibility and published installer/update behavior. The verified remote CI runs are named above; do not extrapolate them to future source changes.

## Next actions, in order

1. Remote Ubuntu/macOS CI is green for `3a06df2`. Retest real Pi steering/follow-up with duplicate text and a genuine late acknowledgement if reproducible; confirm no hang or auto replay. Keep real terminal/session-scan evidence separate from synthetic mocks.
2. Stabilize the reported cursor/modal/queue cases before adding more UI behavior. Record the exact build/source, Pi version, terminal/platform and minimal reproduction; no credentials or private transcripts.
3. Reproduce queue ordering/identity risks in isolated fixtures, then add targeted regressions. Keep protocol behavior separate from presentation changes.
4. Run the relevant automated checks and retest the actual reported UX after rebuilding. Record **what was observed**, not just “fixed.” Keep unresolved items until evidence closes them.
5. Before publishing, verify the intended version-matching commit, supported-platform CI and release artifacts. No release tag/version bump is part of this image-sharing task.

Details live in `docs/terminal.md`, `docs/error-handling.md`, `docs/architecture.md`, and the named tests. Those documents describe intended/current implementation; they are not additional proof that a reported bug is resolved.
