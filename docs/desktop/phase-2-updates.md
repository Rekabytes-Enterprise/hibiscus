# Phase 2: updates and polish

Goal: the desktop app updates itself safely and feels finished. Requires [Phase 1](phase-1-mvp.md).

Back to the [desktop plan](README.md).

## Auto-updates

- Tauri updater plugin, signed with a Tauri key pair generated once by the maintainer. This needs no Apple or Microsoft account.
  - Private key and its password: GitHub Actions secrets (`TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`), used only by the desktop release jobs. Keep an offline backup: losing it means users must reinstall by hand.
  - Public key: compiled into the app.
- The release jobs add a `.sig` file for each updater bundle (`Hibiscus_aarch64.app.tar.gz`, the AppImage) and a `latest.json` manifest. All of them go into `SHA256SUMS` and provenance.
- The app reads `latest.json` from the stable release URL (`releases/latest/download/latest.json`), checks the signature, and asks the user before installing. No silent install.
- `latest/download` redirects are cached per file for a short time after publishing (seen with v0.3.29). The updater must treat a manifest or bundle that does not match its signature as "try later", not as an error dialog.
- `.deb` installs do not self-update; the app shows "a new version is available" with a link instead.

## Polish

- Remember window size and position, and the last project per window.
- System notification when a long run finishes while the window is in the background.
- Menu: New window, Open folder, Settings, Check for updates.
- Settings screen for what the TUI keeps in config (default model, thinking level) using engine commands, not by editing `config.json` from the frontend.
- Keyboard shortcuts matching the TUI where they make sense.

## Tests

- Updater: a test manifest with a bad signature is rejected; a valid one from a local server is accepted (in `desktop/src-tauri` tests).
- Release workflow: the `latest.json` URLs point at files that exist in the same draft.

## Done when

- Installing version N and publishing N+1 offers the update on macOS and on the AppImage, and the update installs and relaunches.
- A tampered bundle is refused.

## Out of scope

Windows; paid signing; telemetry. The opt-in install counting discussed for the Rooxcode download site is a separate project.
