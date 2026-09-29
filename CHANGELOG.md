# Changelog

All notable changes to Hibiscus are documented here.

## [Unreleased]

## [0.3.2] - 2026-09-28

### Added

- Full-screen `@path` completion searches local files and directories at the composer cursor, with quoted paths for spaces and continued directory completion. Enter or Tab inserts a path without sending the prompt or reading file contents. Search runs off the UI thread through bounded `fd` results where available, with a direct-directory fallback. Pi receives ordinary prompt text and may choose to read the referenced file; this is not Pi's CLI `@file` attachment expansion.

### Verification limits

- Synthetic file-picker/PTY tests check inserted prompt text and that file contents are not silently sent. Real-provider, physical-terminal and macOS acceptance remain pending; a dev push is not a tagged release.

## [0.3.1] - 2026-09-28

### Added

- Render completed assistant Mermaid code blocks as Unicode diagrams in full-screen chat after Pi settles, using the selected Pi installation's optional diagram renderer. Keep Markdown visible while streaming and as fallback when a diagram cannot be rendered or fit the transcript.

### Fixed

- Lay out Markdown tables as aligned, width-aware columns with cell wrapping and per-row separators; accept pipe-delimited model output without a separator line. Use rounded Unicode corners and raspberry-colored borders in color terminals.

### Verification limits

- Diagram replacement and table layout are covered by synthetic tests; live-provider, physical-terminal and macOS presentation remain to be checked. A `dev` push does not publish release artifacts.

## [0.3.0] - 2026-09-28

### Added

- Experimental interactive `/loop <goal>` drives Pi through repeated settled runs, temporarily bypasses Hibiscus's shell approval gate, and instructs Pi to provision browser UAT tooling on the user's host. Esc and reported blockers restore ordinary approvals. Both modes use an internal `loop_status` tool signal rather than displaying a magic completion marker and keep the between-turn footer raspberry until the loop exits. Opt-in strict mode freezes the initial named checklist and requires passing Playwright JSON tests and screenshot/console attachments per step, plus a read of an attached image. A separate reporting-only Pi SDK reviewer checks the immutable original request and evidence each turn, permits missing steps to be added before locking scope, and fails closed on unavailable verdicts. Consolidate missing UAT criteria in targeted continuation prompts; show checklist and verified UAT progress separately, fold repeated completion-only chat rows locally, and report stalled loops after targeted recovery. This still does not prove that generated tests meaningfully cover every behavior.

### Changed

- Loop instructions now request Playwright failure capture before the first browser run (`only-on-failure` in guided mode, `on` in strict mode, with traces and optional failure video), plus original and separately annotated failure screenshots, failed-control/locator identification, expected-versus-actual results, a Markdown failure summary, and post-fix screenshots. Model-driven guidance applies to both modes and continuations; unavailable annotations must be reported, not fabricated.
- Default `/loop <goal>` now uses flexible Pi-guided test-and-fix instructions with explicitly model-reported completion. Existing Playwright evidence contracts and independent reviewer are opt-in through `/loop --strict <goal>`; guided mode needs no reviewer SDK or prescribed artifacts.
- Accept inline and duplicate Playwright attachments; retain accepted evidence with explicit stale reasons based on bounded source comparison. Return report validation results directly to Pi and provide bounded test-source, console/log and historical failure excerpts to the reviewer.
- Reviewer verdicts use Pi-native schema-defined `submit_verdict` tool arguments instead of parsing JSON prose, with one bounded format correction and explicit handling of provider errors, cancellation, truncation and incompatible SDKs. No custom Pi installation or global configuration change.
- Accept `/loop` UAT evidence from saved Playwright JSON reports through `loop_status submit_evidence`; resolve relative artifact paths, check report freshness, preserve results during known read-only log checks, and send validated evidence metadata/missing checks to the independent reviewer.
- Add a separate Hibiscus operational-guidance prompt section alongside identity guidance: inspect safely, ask about material ambiguities, use conservative defaults for minor reversible details, and report verification honestly. Keep Pi's original prompt and tools; model compliance is not guaranteed.
- Dangerous-command Always Allow can authorize displayed per-command prefix patterns for the current directory/session (for example, `git push *`), including changed comments and arguments. Compound commands are reviewed per component where the syntax is simple; complex shell syntax stays exact. Parallel pending decisions reuse a matching grant, and Deny rejects queued approvals. Broad patterns can include force flags or other targets—review the scope before approving.

## [0.2.5] - 2026-09-27

### Added

- Full-screen transcript drag selection and clipboard copying, including Ctrl+C to copy an active selection. On an empty selection, two Ctrl+C presses within two seconds quit instead of one.
- Bracketed multiline terminal paste inserts text into the composer without submitting the first line.

### Changed

- Improve layouts below 60 terminal columns with smaller margins, a compact header, and complete activity/Goal fields instead of clipped usage text. Wider layouts retain their previous spacing.
- Dangerous-command approval waits for an explicit choice instead of auto-denying after 60 seconds. Deny remains the default, Esc cancels, and unrelated explicit dialog timeouts still apply.

### Verification limits

- Clipboard and mobile terminal presentation still require physical-terminal retesting. The approval gate remains a best-effort guard, not a sandbox.

## [0.2.4] - 2026-09-26

### Added

- Pi-confirmed thinking level in the header and cumulative context/input/output/cache tokens plus approximate cost in the single footer row beside activity and optional Goal. Usage refreshes only at confirmed idle boundaries; unsupported Pi stats and null context do not invent values.

### Changed

- Abbreviate displayed token counts and manual compaction notices with decimal K/M/B/T units (e.g. 200K/1M); shorten cache reads to `Cache ~value`, omit cache writes, and cap displayed USD cost at two decimals (with `<$0.01` for small nonzero costs), without changing Pi's accounting.

## [0.2.3] - 2026-09-26

### Added

- `/thinking [level]` to choose only Pi-reported reasoning levels for the selected model, and `/compact [instructions]` to let Pi summarize older context with live status, Esc cancellation and explicit response correlation.

## [0.2.2] - 2026-09-25

### Fixed

- Preserve Hibiscus identity guidance when Pi's `before_agent_start` event has no structured prompt sections. Fall back to Pi's existing rendered prompt or append option instead of throwing an extension warning.

## [0.2.1] - 2026-09-25

### Fixed

- Route Pi stderr warnings to a bounded, sanitized diagnostic row below the full-screen composer/status instead of allowing raw output to overwrite it. Preserve drafts, cursor layout, dialog focus, reconnect routing and ordinary stderr in line/piped modes; provider settings are unchanged.

## [0.2.0] - 2026-09-25

### Added

- Standard MIT license with Rekabytes Enterprise attribution and a fork/feature-branch contribution guide targeting `dev`.
- Repeatable formatter, allocation and session-list benchmarks, plus a large-history paste regression and performance review.
- Correlate delayed steering acknowledgements after final settlement without hanging or replaying, while still waiting for nonempty Pi queues to drain.
- Full-screen `/sessions` and `/continue` now scan Pi session files off the UI thread, showing a cancellable loading notice and buffering typing for the composer; line-mode/startup listing is unchanged.
- Optional isolated Linux input-wake profiler and shared input/RPC activity notification, reducing quiet-run key-to-output latency while retaining the timed animation fallback.
- Optional Linux-only synthetic memory profiler with per-process RSS/high-water counters, isolated mock Pi/clipboard fixtures, and logical live/peak heap-byte benchmarks. No runtime behavior changes are part of this profiling addition.

### Changed

- Guide identity answers to introduce Hibiscus as the terminal assistant powered by Pi, without replacing Pi's system prompt or misidentifying the model/provider.
- Full-screen chat, composer, status and docked panels now grow with the terminal instead of remaining capped at 100 columns; drafts and cursor layout reflow on resize.
- Validate but do not materialize incoming user image-echo base64 when only text and attachment counts are needed; leave Pi's saved messages and other RPC event payloads intact.
- Validate but do not materialize the unused `partialResult` JSON subtree of `tool_execution_update`; preserve all other event fields, errors, approvals, queue ordering and settlement.
- Share staged image values between the composer and recovery slot, avoiding a second base64 allocation on queued rejection while keeping Pi's JSONL wire format and explicit `/restore` behavior.
- Bound RPC wire-record/backlog memory and record counts with explicit overload failure, nonblocking/deadline-controlled writes, partial-send cancellation and reviewable uncertain queued drafts. Add synthetic duplex/overload regressions and opt-in queue metrics.
- Cache transcript layout independently of terminal row painting, reuse completed message blocks, and coalesce scrolled row counting.
- Batch available printable input and read terminal bytes in chunks without changing control-key or UTF-8 handling.
- Avoid per-character Markdown string allocations, repeated front-shifting during wrapping, and repeated failed link searches.
- Cache unchanged session metadata with bounded memory, file-identity checks, invalidation and periodic expiry.
- Borrow images during RPC serialization, move response data/base64 buffers, and queue compact raw incoming records for consumer-side parsing. Buffered output errors do not implicitly retry commands.

## [0.1.7] - 2026-09-25

### Added

- Editable full-screen input during Pi runs: Enter requests steering, and Ctrl+Q (WSL) or Alt+Enter queues a follow-up, including image attachments.
- A pending-message panel that distinguishes queue acceptance from delivery; user rows are inserted on Pi's user-message event.
- An explicit `goal` checklist tool with model-reported completion counts and a compact progress indicator beside the live status.
- Shared docked modal components for approvals, selections, confirmations, text input, and supported auth prompts.

### Changed

- Composer editing supports arrow navigation, Home/End, and insertion/deletion at the cursor across multiline drafts.
- The terminal UI uses an aubergine background, a latest-file exploration indicator, checked Read/Bash counts, and per-edit diff previews.
- Full-screen rendering caches rows and batches streamed text updates, with synchronized output where supported and a steady-cursor request. `HIBISCUS_NO_SYNC_UPDATE=1` disables synchronized markers.
- Project state and memory records now separate implementation, automated evidence, user reports, and unresolved risks instead of accumulating historical test claims.

### Verification limits

- Automated checks use unit tests, mocked Pi/SDK subprocesses, and PTYs. Real terminal flicker, provider queue ordering, auth flows, and macOS behavior still require targeted retesting.
- Goal percentages reflect checklist updates supplied by the model, not independently verified work. The approval gate remains a best-effort guard, not a sandbox.

## [0.1.6] - 2026-09-25

### Added

- A best-effort approval gate for recognized dangerous agent shell commands, with Deny, Allow once, and session-scoped Always Allow for the exact command and working directory. Routine `read`, `edit`, `write`, and `bash` remain automatic.
- `/restore` for reviewing a failed prompt and its images, plus `/reconnect` for reconnecting Pi without automatically replaying commands.
- In-chat `/logout` for choosing and removing a stored Pi provider credential with explicit confirmation.
- Raspberry flower logo, illustrated terminal preview, release-download badge, and a streamlined README.

### Fixed

- Provider failures such as rate limits and quota exhaustion no longer close interactive chat; successful Pi retries no longer inherit earlier attempt failures. One-shot and piped errors retain nonzero exit statuses.
- `/new` opens a blank full-screen chat after Pi confirms session creation, without deleting earlier saved sessions.
- Restored session history distinguishes user and agent messages; user rows have a subtle tint and agent replies display a `hibi` heading.

### Notes

- The approval gate is not a sandbox: scripts, indirect tool effects, and ordinary file edits can still change data. Deny is the default; no interactive approval means no permission for a flagged tool call.
- Logout removes a stored credential only. It does not revoke provider tokens or unset environment/configuration credentials.

## [0.1.5] - 2026-09-25

### Added

- Multiline composer input with wrapping, scrolling, and Ctrl+Enter support.
- Clipboard image attachments with platform-specific paste shortcuts.
- Image-only and text-plus-image prompts through Pi RPC.

### Changed

- Improved full-screen composer layout and attachment controls.
- Added bounded, cancellable clipboard helper processes.
- Added image placeholders to history instead of exposing image data.

## [0.1.4] - 2026-09-25

### Added

- `/models` support for models across all configured providers.
- Provider-aware model selection and switching without reauthentication.

## [0.1.3] - 2026-09-25

### Changed

- Codex browser sign-in now opens the validated OAuth URL with the platform browser on macOS.
- Improved OAuth completion handling and fallback behavior.

## [0.1.2] - 2026-09-25

### Changed

- Incremental patch release following the initial release and installer work.

## [0.1.1] - 2026-09-24

### Added

- Checksum-verified prebuilt installation for Linux and macOS.
- Automatic Pi installation when Pi is not already available.
- `hibiscus update` with atomic, checksum-verified updates.
- Background update checks for full-screen prebuilt installations.

### Changed

- Restricted Hibiscus-launched Pi instances to built-in tools and disabled extensions.
- Improved session handling, authentication handoffs, and terminal test reliability.

## [0.1.0] - 2026-09-24

### Added

- Initial Rust CLI for chatting with Pi through JSONL RPC.
- Full-screen terminal chat with Markdown rendering, progress indicators, tool activity, and scrolling.
- Persistent Pi sessions, session resume, `/new`, `/sessions`, `/models`, and authentication flows.
- Line-mode fallback for non-interactive terminals.

[Unreleased]: https://github.com/Rekabytes-Enterprise/hibiscus/compare/v0.3.2...HEAD
[0.3.2]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.2
[0.3.1]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.1
[0.3.0]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.3.0
[0.2.5]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.5
[0.2.4]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.4
[0.2.3]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.3
[0.2.2]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.2
[0.2.1]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.1
[0.2.0]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.2.0
[0.1.7]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.7
[0.1.6]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.6
[0.1.5]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.5
[0.1.4]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.4
[0.1.3]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.3
[0.1.2]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.2
[0.1.1]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.1
[0.1.0]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.0
