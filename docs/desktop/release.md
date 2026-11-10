# Desktop release

`.github/workflows/release-desktop.yml` builds the desktop app for the same `v*` release tags as `release.yml`, and adds its files to the draft release that `release.yml` creates. It is a separate workflow so the CLI release, its provenance script and its tests stay unchanged.

## What it builds

| File | Platform | Built on |
|---|---|---|
| `hibiscus-desktop-aarch64-apple-darwin.dmg` | macOS, Apple Silicon | `macos-15` |
| `hibiscus-desktop-aarch64-apple-darwin.app.tar.gz` and `.sig` | macOS update, and its updater signature | `macos-15` |
| `hibiscus-desktop-x86_64.AppImage` | Linux x86_64 | `ubuntu-22.04`, the oldest supported Ubuntu, so the AppImage runs on more distributions |
| `hibiscus-desktop-x86_64.AppImage.sig` | updater signature for the AppImage | `ubuntu-22.04` |
| `latest.json` | the update manifest installed apps read | |
| `SHA256SUMS-desktop` | checksums of the files above | |
| `hibiscus-desktop-provenance.sigstore.json` | signed build provenance for the files above | |

With the CLI's files, a full release has 14 files.

The app's version comes from the root `package.json` (`"version": "../../package.json"` in `tauri.conf.json`), which `scripts/bump-version.sh` already updates. The bundle ID is `com.rooxcode.hibiscus`.

## How a release runs

1. **verify** takes the commit that triggered the run and accepts it only if it is on `main`. A manual build-only run may also use `dev`.
2. **macos_arm64** and **linux_x64** check out that same triggering commit, without credentials, confirm it is the authorized one, run `npm ci`, and build with Tauri. macOS also packs the `.app` into the `.app.tar.gz` the updater installs. Each job writes a receipt listing the SHA-256 of its files, and records the receipt's own digest.
3. **sign_updates** checks the build receipts, then signs the `.app.tar.gz` and the AppImage with the updater key and writes `latest.json`. It is the only job that sees a secret. It has no checkout and runs no repository code: only the pinned Tauri CLI, installed without install scripts. It fails at once if the key secret is missing. It records a receipt for the files it adds.
4. **provenance** checks all three receipts against their digests and every file against its receipt, signs the files with `actions/attest`, and writes `SHA256SUMS-desktop`.
5. **publish** waits up to 30 minutes for `release.yml` to create the draft release, then uploads the eight files into it. It never creates a release and refuses one that is already published.

Only `publish` can write to the repository, only `provenance` can sign provenance, and only `sign_updates` holds the updater key.

To try the build without releasing, run the workflow by hand on `main` or `dev`. The two files are kept as run artifacts for 5 days and nothing is published. To rebuild a release, re-run the failed jobs of that tag's run; the workflow only ever builds the commit that triggered it.

## Updates

Installed apps check `releases/latest/download/latest.json` on launch and every 6 hours. GitHub only serves published releases there, so a draft never reaches users. When a newer version is listed, a banner offers **Update**. The app downloads the file for its platform, checks the signature against the public key in `tauri.conf.json`, installs it over itself and restarts.

### Updater signing key

The update files are signed with a Tauri updater key, separate from the provenance signature. It is set up once:

1. Generate the key pair and pick a password:

   ```bash
   cd desktop && npm run tauri signer generate -- -w ~/.tauri/hibiscus.key
   ```

2. Add two repository secrets: `TAURI_SIGNING_PRIVATE_KEY`, the contents of `~/.tauri/hibiscus.key`, and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. Put the contents of `~/.tauri/hibiscus.key.pub` in `plugins.updater.pubkey` in `desktop/src-tauri/tauri.conf.json`. The public key is safe to commit.
4. Back up the private key and its password. If they are lost, installed apps can never update again, and users have to reinstall by hand.

Builds never see the key, so local builds and the release build jobs work the same way. Only `sign_updates` signs.

## Opening the unsigned files

The app is not signed with an Apple Developer ID or notarised.

### macOS

Open the `.dmg` and drag Hibiscus to Applications. The first time you open it, macOS says "Hibiscus" Not Opened, because Apple could not verify it. Click **Done**, not "Move to Trash", then:

1. Open **System Settings → Privacy & Security**.
2. Scroll down to **Security**. Next to "Hibiscus" was blocked to protect your Mac, click **Open Anyway**.
3. Confirm with your password or Touch ID.
4. Open Hibiscus again and click **Open Anyway** in the dialog.

macOS remembers this, so later launches open normally. The **Open Anyway** button in Settings only appears for about an hour after the blocked launch; if it's gone, open Hibiscus once more and go back to Settings.

Since macOS 15 (Sequoia), right-clicking the app and choosing Open no longer skips this check.

If you prefer the terminal, removing the download mark has the same effect:

```bash
xattr -d com.apple.quarantine /Applications/Hibiscus.app
```

### Linux

Make the AppImage executable, then run it:

```bash
chmod +x hibiscus-desktop-x86_64.AppImage && ./hibiscus-desktop-x86_64.AppImage
```

### Checking a download

Check a download against `SHA256SUMS-desktop` from the same release:

```bash
shasum -a 256 -c SHA256SUMS-desktop --ignore-missing
```
