# Phase 1: desktop MVP

Status: **in progress.** Done: the app under `desktop/` (glue, all screens below, icons, frontend tests). Left: release jobs, `install-desktop.sh`, a desktop CI job, the docs/installation section, and confirming the bundle ID.

Goal: a Hibiscus desktop app for macOS Apple Silicon and Linux x86_64, shipped in the normal release. Requires [Phase 0](phase-0-library.md) and the [decisions](README.md#phase-1-decisions).

Back to the [desktop plan](README.md).

## Repository layout

```
desktop/
  package.json          frontend build (pinned lockfile)
  src/                  frontend (UI only, no secrets, no file access)
  src-tauri/
    Cargo.toml          depends on hibiscus = { path = "../.." }
    Cargo.lock          own lockfile and an empty [workspace], so Tauri never enters the CLI's dependency tree
    tauri.conf.json
    capabilities/       Tauri permissions
    src/main.rs         thin glue: Tauri commands → hibiscus::desktop
```

The CLI crate stays dependency-free of Tauri; `cargo build` at the repository root builds exactly what it builds today.

## Backend glue (`src-tauri/src/main.rs`)

Tauri commands, each a few lines over `hibiscus::desktop`:

| Command | Calls |
|---|---|
| `open_project(folder, latest, session)` | `Engine::start(start, folder)`; one engine for the window (more windows in Phase 2) |
| `send(command)` | `Engine::send`; returns the request id |
| `save_api_key(provider, key)` | `desktop::save_api_key` |
| `sessions(folder)` | `desktop::sessions` |
| `close_project()` | `Engine::shutdown` |
| `providers()` | `desktop::providers` (includes signed-out providers) |

A reader thread per engine calls `next_event` and forwards each event to its window with `window.emit("engine", event)`. Closing the window shuts the engine down and waits for its tools to stop, the same as quitting the TUI.

## Frontend screens

1. **Open project:** recent folders plus "Open folder…" (native dialog). Required, since the engine works in that folder.
2. **Chat:** streamed replies (`message_update`), thinking, tool calls with their output (`tool_execution_start`/`end`), queue and steering (`queue_update`), Abort, image paste.
3. **Models:** the provider tabs and search from the TUI's `/models`, built from `get_available_models`; signed-out providers hidden, as in the TUI.
4. **Login:** provider list (`desktop::providers`), API-key field, "create one at …" link from `key_page`. The key goes straight to `save_api_key`; the frontend never stores it. Saving restarts the engine on the same session, as the TUI reconnects after `/login`. `openai-codex` signs in through the browser with `desktop::CodexSignIn`, and any signed-in provider can sign out (`desktop::sign_out`). The built-in `rooxcode` provider is not listed.
5. **Sessions:** list, resume (`switch_session`), new (`new_session`).
6. **Dialogs:** not needed yet; the native engine sends no `extension_ui_request` today (see [protocol](protocol.md)).
7. **Errors:** `provider_error` and failed responses shown as they are; no automatic retry beyond what the engine already does.

## Security settings

- Tauri capabilities: only the app's own commands plus the folder dialog. No `shell`, `fs` or `http` plugins in the frontend; the engine does all file and network work in Rust, under its existing limits.
- Strict CSP, all frontend assets bundled, no remote content, no `eval`.
- External links (key pages) open in the user's browser through the opener, not inside the app.
- API keys stay in `auth.json` (`0600`), never in frontend storage or logs.

## Packaging

| Target | Runner | Files |
|---|---|---|
| `aarch64-apple-darwin` | `macos-15` | `Hibiscus_<ver>_aarch64.dmg`, `Hibiscus_aarch64.app.tar.gz` |
| `x86_64-unknown-linux-gnu` | `ubuntu-22.04` | `Hibiscus_<ver>_amd64.AppImage`, `Hibiscus_<ver>_amd64.deb` |

- macOS: ad-hoc signature (`codesign --sign -`), as `release.yml` already does for the CLI.
- Linux: build on Ubuntu 22.04 so the glibc baseline stays old. The `.deb` depends on `libwebkit2gtk-4.1-0` (Ubuntu 22.04+, Debian 12+); the AppImage carries its own libraries.

## Release workflow (`release.yml`)

- Two new jobs, `desktop_macos_arm64` and `desktop_linux_x64`, using the same frozen source commit, pinned actions and `--locked` builds as the CLI jobs.
- `provenance` and `publish` gain them in `needs:`, so the desktop files land in **the same draft release**, in `SHA256SUMS` and in `hibiscus-provenance.sigstore.json`.
- The draft-then-publish process does not change. Publishing before the workflow finishes still breaks the upload, as it did for v0.3.29.

## Install script (`install-desktop.sh`)

Same style as `install.sh`: download from the release, check `SHA256SUMS`, refuse on mismatch.

- macOS: unpack `Hibiscus.app` into `~/Applications`. Downloaded with `curl`, so no quarantine flag and no Gatekeeper prompt.
- Linux: put the AppImage in `~/.local/bin`, add a `.desktop` entry and icon under `~/.local/share`.
- `tests/install-desktop.sh`, matching `tests/install.sh`.

## Docs

- `docs/installation.md`: desktop section with the script, the `.dmg` "Open Anyway" steps and the Linux packages.
- `README.md`: short desktop paragraph and link.
- `CHANGELOG.md` entry.

## Tests

- `hibiscus` core tests unchanged and passing.
- `desktop/src-tauri`: `cargo test` for the glue (command → engine round trip with the fake provider).
- Frontend: unit tests for the event reducer (stream, tool, queue, error events → screen state).
- CI job that builds the desktop app on both runners on pull requests that touch `desktop/` or `src/desktop.rs`.
- Manual checklist before the first release: open a project, chat with tools, abort, switch models, log in with a key, resume a session, answer a dialog, quit with a running tool.

## Done when

- A tagged draft release contains the four desktop files next to the CLI archives, all in `SHA256SUMS` and provenance.
- `install-desktop.sh` installs and launches the app on macOS 15 Apple Silicon and Ubuntu 22.04/24.04.
- The manual checklist passes on both.

## Out of scope

Auto-updates, window state, notifications (Phase 2); Windows; Intel Mac; Linux ARM; signing with paid accounts.
