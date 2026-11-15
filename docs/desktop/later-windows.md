# Later: Windows

Draft. Nothing here is built yet. Start only after macOS and Linux are stable.

Back to the [desktop plan](README.md).

The goal is a Windows x64 installer, `hibiscus-desktop-x86_64-pc-windows-msvc-setup.exe`, built by `release-desktop.yml` for the same `v*` tags as the macOS and Linux files and added to the same draft release.

## Why it is separate

The engine does not compile on Windows today. About 21 files rely on Unix-only features:

- the Unix socket between the UI and the engine (`client/bridge/`, `client/transport.rs`, `client/embed.rs`, `engine/agent.rs`);
- `/bin/sh` for the shell tool (`engine/tools.rs`) and the dev server (`devserver.rs`);
- process groups and `libc` signals for stopping tools;
- `std::os::unix` file modes (`0600`) for `auth.json`, sessions, checkpoints and extensions, imported without a `cfg` gate;
- raw terminal mode for the TUI (`ui/terminal.rs`);
- the desktop app's login-shell environment import (`desktop/src-tauri/src/main.rs`), which runs `$SHELL -ilc`.

## Decision: unsigned, internal first

The first Windows builds are unsigned and go to the team only, for testing and feedback. Code signing waits until the app is ready for a public release. See [Code signing](#code-signing) for the options when that time comes.

| Stage | Who gets it | How |
|---|---|---|
| Internal | The team | `windows_x64` runs only on manual `workflow_dispatch` runs. A team member runs **Desktop release** by hand on `dev` or `main` and downloads the `desktop-x86_64-pc-windows-msvc` artifact from the run page. Only people with access to the repository can download it. Tag releases keep shipping macOS and Linux only, so the release stays at 14 files. |
| Public | Everyone | Remove the `workflow_dispatch` condition. `windows_x64` runs on `v*` tags and the release grows to 16 files. Still unsigned unless signing is set up by then. |
| Signed | Everyone | Add Authenticode signing. |

During the internal stage:

- Artifacts are deleted after 5 days. Run the workflow again for a fresh build.
- Internal builds don't update themselves, because `latest.json` only lists Windows from the public stage. Testers install each new build over the old one.
- Testers see SmartScreen's "Windows protected your PC" and click **More info**, then **Run anyway**. Testers with Smart App Control turned on can't run it at all. They have to turn Smart App Control off, and Windows only allows that by resetting the PC, so test on machines where it is already off.

## Order of work

1. **Engine port**, one PR to `dev`. Windows CI must pass before step 2 lands.
   1. Replace the socket pair with a named pipe or an in-process channel, keeping the same limits and deadlines.
   2. Run the shell tool on PowerShell, or Git Bash when it is installed. Stop tools with Job Objects instead of process groups.
   3. Gate the `std::os::unix` code. Keep private files private with Windows ACLs, and store data under `%APPDATA%`.
   4. Skip the login-shell environment import on Windows. A Start-menu launch already has the user's environment.
   5. Ship the TUI on Windows later, or with console mode. The desktop app does not need it.
   6. Add `windows-latest` to the CI test matrix.
2. **Packaging and release**, one PR to `dev`. See [Release workflow](#release-workflow). `windows_x64` starts with `if: github.event_name == 'workflow_dispatch'` for the internal stage.
3. **Code signing**, once a certificate is chosen. See [Code signing](#code-signing).

Keep the `workflow_dispatch` condition until the port passes and the team signs off. Without it, a failing Windows build stops `publish` from adding the desktop files for every platform.

## Release workflow

### App config

- `desktop/src-tauri/icons/icon.ico`, generated from `icon.png` with `npx tauri icon`, and listed in `bundle.icon`.
- `"nsis"` added to `bundle.targets`. The output is an `.exe` installer. No `.msi`.
- A `bundle.windows` block:

  ```json
  "windows": {
    "webviewInstallMode": { "type": "downloadBootstrapper" },
    "nsis": { "installMode": "currentUser" }
  }
  ```

  `currentUser` installs under `%LOCALAPPDATA%` without admin rights, so updates install silently. The bootstrapper installs WebView2 only when it is missing. Windows 10 and 11 normally have it.

### Bundled agent-browser

`desktop/scripts/agent-browser.sh` gets an `x86_64-pc-windows-msvc` case with the Windows asset and its SHA-256. On Windows the file is saved as `agent-browser-x86_64-pc-windows-msvc.exe`, because Tauri's `externalBin` adds `.exe` there. The script stays `sh`: the `windows-latest` runner has Git Bash, so `npm run tauri` can still call it.

### New job

`windows_x64` runs beside `macos_arm64` and `linux_x64`, and works the same way:

```yaml
  windows_x64:
    # Internal stage: manual runs only. Remove for the public stage.
    if: github.event_name == 'workflow_dispatch'
    needs: verify
    runs-on: windows-latest
    timeout-minutes: 60
    permissions:
      contents: read
    outputs:
      sha256: ${{ steps.receipt.outputs.sha256 }}
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - name: Check out the authorized commit
        shell: bash
        env:
          AUTHORIZED_COMMIT: ${{ needs.verify.outputs.source_commit }}
        run: test "$(git rev-parse HEAD)" = "$AUTHORIZED_COMMIT"
      - uses: actions/setup-node@949feb2413d6458794dcd2491c4babbbce0c15c1 # v7.1.0
        with:
          node-version: '24'
      - uses: dtolnay/rust-toolchain@b725eda525362e7cf34af9a7243cac1e3f01a7ec # stable
        with:
          toolchain: 1.99.1
      - name: Build the app
        shell: bash
        working-directory: desktop
        run: |
          npm ci
          npm run tauri -- build --ci --bundles nsis
      - name: Name the file and record its digest
        id: receipt
        shell: bash
        run: |
          shopt -s nullglob
          built=(desktop/src-tauri/target/release/bundle/nsis/*-setup.exe)
          test "${#built[@]}" -eq 1
          mkdir -p dist
          cp "${built[0]}" dist/hibiscus-desktop-x86_64-pc-windows-msvc-setup.exe
          (cd dist && sha256sum hibiscus-desktop-* > receipt-windows)
          printf 'sha256=%s\n' "$(sha256sum dist/receipt-windows | cut -d ' ' -f 1)" >> "$GITHUB_OUTPUT"
      - uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: desktop-x86_64-pc-windows-msvc
          path: dist/*
          if-no-files-found: error
          retention-days: 5
```

### Changes to the later jobs

These changes are for the public stage only. During the internal stage, leave `sign_updates`, `provenance` and `publish` as they are. GitHub skips every job that `needs` a skipped job, so if they listed `windows_x64` while it is limited to manual runs, tag releases would publish nothing.

| Job | Change |
|---|---|
| `sign_updates` | `needs` adds `windows_x64`. Expects 7 files (4 built, 3 receipts). Checks `receipt-windows` (1 line). Signs the `-setup.exe`: on Windows the Tauri updater runs the NSIS installer itself. `latest.json` gets a `windows-x86_64` platform with that file's URL and signature. `receipt-updates` has 4 lines. |
| `provenance` | `needs` adds `windows_x64`. Expects 13 files (8 release files, 4 build receipts, `receipt-updates`). The upload list adds the `.exe` and its `.sig`. |
| `publish` | Expects 10 files. |

A full release grows from 14 files to 16. Update the count in [Desktop release](release.md) when this lands, and publish the draft only once all 16 are there.

### Opening the unsigned installer

Until the installer is signed, Windows SmartScreen shows "Windows protected your PC". Users click **More info**, then **Run anyway**. On Windows 11 with Smart App Control turned on, unsigned apps are blocked and there is no Run anyway button. Those users need a signed build.

## Code signing

Windows checks two things, and both are separate from the Tauri updater signature:

- **Authenticode signature** on `Hibiscus.exe`, the uninstaller and the `-setup.exe`. It shows "Rekabytes Enterprise" as the publisher in place of "Unknown publisher".
- **SmartScreen reputation.** It builds up per certificate as people download and run the signed files. Since 2024 an EV certificate no longer skips this, so EV is not worth its extra cost here.

### Options

| Option | Cost | Fits CI | Notes |
|---|---|---|---|
| Azure Trusted Signing | about $10/month | Yes, signs with an Azure login, no key file | Microsoft decides eligibility by the organization's country and age. Check that Rekabytes Enterprise qualifies before choosing it. |
| OV certificate with a cloud key (SSL.com eSigner, Certum SimplySign, DigiCert KeyLocker) | about $130–$400/year | Yes, through the vendor's signing tool | Since June 2023, certificate authorities only issue code-signing keys on hardware or a cloud HSM, so a `.pfx` file in a secret is no longer an option. |
| OV certificate on a USB token | similar | No | Signing needs the token plugged in, so releases would be signed by hand. Not recommended. |
| Unsigned | free | Yes | The first beta can ship this way. See [Opening the unsigned installer](#opening-the-unsigned-installer). |

Recommendation: Azure Trusted Signing if Rekabytes Enterprise is eligible. Otherwise, an OV certificate with a cloud key.

### How signing fits the workflow

Tauri signs on Windows through `bundle.windows.signCommand` in `tauri.conf.json`. Tauri calls it for each file it builds: the app `.exe`, the bundled `agent-browser.exe`, the uninstaller and the `-setup.exe`. The command is the signing tool for the chosen option, run with `%1` as the file path.

The app `.exe` is packed inside the installer, so it has to be signed during the build, before NSIS runs. That means the `windows_x64` job holds the signing credentials while it runs `npm ci` and builds repository code. Today only `sign_updates` holds a secret, and it runs no repository code. Two ways to handle it:

- **Sign in the build job.** Every file is signed, including the installed `Hibiscus.exe`. The signing credentials are exposed to the build's dependencies. Limit the damage: use a credential that can only sign (an Azure identity with only the Trusted Signing role, or a vendor sub-account), put it in a `windows-signing` GitHub environment that only `v*` tags on `main` may use, and keep `npm ci` locked to `package-lock.json`.
- **Sign only the installer, in a separate job.** A new `sign_windows` job, with no checkout, signs the finished `-setup.exe`, like `sign_updates`. The secret never meets repository code, but the installed `Hibiscus.exe` stays unsigned, so Smart App Control still blocks it.

Recommendation: sign in the build job, with the limits above. A half-signed app does not fix the Smart App Control case, and that case is the main reason to sign.

Order matters. The Authenticode signature changes the file, so `sign_updates` must sign the `-setup.exe` for the updater after the build job has signed it. The workflow already runs in that order.

### Setup, once a certificate is chosen

1. Get the certificate or the Trusted Signing account in the name Rekabytes Enterprise.
2. Create the `windows-signing` environment in the repository settings, limited to `v*` tags, and add the signing secrets to it.
3. Add `environment: windows-signing` and the signing tool's setup step to `windows_x64`.
4. Set `bundle.windows.signCommand` in `tauri.conf.json`.
5. After the first signed release, check with `signtool verify /pa /v` that all four files carry the signature, and that the installer shows the publisher name.
6. Manual `workflow_dispatch` runs have no access to the environment, so they build unsigned. `signCommand` must skip signing when its credentials are missing, instead of failing the build.
