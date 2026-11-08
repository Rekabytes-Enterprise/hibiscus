# Desktop app plan

Status: Phase 0 done; Phase 1 in progress (the app runs from source; packaging in releases is next). The engine protocol is in [protocol.md](protocol.md).

Hibiscus gets a desktop app for **macOS Apple Silicon** and **Linux x86_64**. Windows comes later (see [Windows](later-windows.md)). The CLI and TUI stay as they are; the desktop app is an extra way to use the same engine.

## Phases

| Phase | Doc | Result | Users see a change? |
|---|---|---|---|
| 0 | [Library API](phase-0-library.md) (done) | The engine can be driven from code other than the terminal UI | No |
| 1 | [Desktop MVP](phase-1-mvp.md) | Tauri app, `.dmg` and `.AppImage`/`.deb` in the release, desktop install script | Yes |
| 2 | [Updates and polish](phase-2-updates.md) | Signed auto-updates, window state, notifications | Yes |
| Later | [Windows](later-windows.md) | Native Windows port and `.msi`/`.exe` | Yes |

Each phase ships on its own and leaves `main` releasable.

## Fixed decisions

- **Framework:** Tauri v2. The engine runs inside the app process, the same way the TUI runs it today. No sidecar binary, no local server, no open port.
- **Platforms:** `aarch64-apple-darwin` and `x86_64-unknown-linux-gnu`. Intel Mac and Linux ARM are not in the first release.
- **Signing:** no Apple Developer or Windows accounts for now. macOS builds get a free ad-hoc signature. Every file is listed in `SHA256SUMS` and covered by the existing sigstore provenance. The updater uses its own Tauri signing key. See [Unsigned distribution](#unsigned-distribution).
- **Products:** this is Hibiscus work only. It does not change the Rooxcode Gateway's code, behaviour or tests; the gateway stays one provider inside the engine.

## Running from source

Needs Node 20+, Rust and, on Linux, the [Tauri system packages](https://v2.tauri.app/start/prerequisites/) (`libwebkit2gtk-4.1-dev` and friends).

```bash
cd desktop
npm install
npm run tauri dev      # opens the app; reloads on frontend changes
npm test               # frontend unit tests
npm run check          # Svelte/TypeScript checks
npm run tauri build    # .app/.dmg on macOS, .AppImage/.deb on Linux, under src-tauri/target/release/bundle/
```

If the macOS `.dmg` step fails with "Finder got an error: AppleEvent timed out", run it as `CI=true npm run tauri build`; that skips the Finder window-layout step, as GitHub Actions does.

The app uses the same `~/.hibiscus` (or `HIBISCUS_HOME`) config, keys and sessions as the terminal app. A Dock or menu launch copies the login shell's environment (PATH, provider key variables) so tools behave as they do in a terminal; set `HIBISCUS_DESKTOP_KEEP_ENV=1` to skip that.

## Phase 1 decisions

1. **UI:** a real GUI on the [engine protocol](protocol.md) (decided).
2. **App name and bundle ID:** Rooxcode-branded (decided). `tauri.conf.json` uses the placeholder `com.rooxcode.hibiscus`; confirm the domain before the first release. Choose once: the updater and macOS settings key off it.
3. **Linux:** `x86_64` only for the first release (decided).
4. **Frontend:** Svelte 5 + TypeScript (decided).

## Unsigned distribution

| Platform | First launch | How the user gets past it |
|---|---|---|
| Linux | No prompt | `chmod +x` the AppImage, or install the `.deb` |
| macOS, installed with `install-desktop.sh` | No prompt (`curl` does not set the quarantine flag) | Nothing |
| macOS, `.dmg` from a browser | "Apple cannot check it for malicious software" | Try to open once, then System Settings → Privacy & Security → **Open Anyway** |

Not possible without an Apple account: Homebrew casks, the Mac App Store, and a warning-free first launch from a browser download. Adding signing later only changes the release workflow, not the app.
