# Changelog

All notable changes to Hibiscus are documented here.

## [Unreleased]

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

[Unreleased]: https://github.com/Rekabytes-Enterprise/hibiscus/compare/v0.2.2...HEAD
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
