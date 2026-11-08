# Later: Windows

Not scheduled. Start only after macOS and Linux are stable.

Back to the [desktop plan](README.md).

## Why it is separate

About 21 files rely on Unix-only features, and none of them work on Windows as written:

- the Unix socket between the UI and the engine (`runtime/bridge.rs`, `runtime/transport.rs`);
- `/bin/sh` for the shell tool;
- process groups and `libc` signals for stopping tools;
- `0600` file permissions for `auth.json` and sessions;
- raw terminal mode for the TUI.

## Work, when it starts

1. Replace the socket pair with an in-process channel (or a named pipe), keeping the same limits and deadlines.
2. Shell tool on PowerShell or `cmd`; stop tools with Job Objects instead of process groups.
3. Private files with Windows ACLs; data under `%APPDATA%`.
4. Console mode for the TUI (or ship Windows as desktop-only at first).
5. `.msi`/`.exe` via Tauri, `x86_64-pc-windows-msvc`, added to the same draft release.

## Signing

Without a code-signing certificate, SmartScreen shows "Windows protected your PC" and users click **More info → Run anyway**. Azure Trusted Signing (about $10/month) removes most of that. The Tauri updater works either way.
