# Changelog

All notable changes to Hibiscus are documented here.

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

[Unreleased]: https://github.com/Rekabytes-Enterprise/hibiscus/compare/v0.1.6...HEAD
[0.1.6]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.6
[0.1.5]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.5
[0.1.4]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.4
[0.1.3]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.3
[0.1.2]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.2
[0.1.1]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.1
[0.1.0]: https://github.com/Rekabytes-Enterprise/hibiscus/releases/tag/v0.1.0
