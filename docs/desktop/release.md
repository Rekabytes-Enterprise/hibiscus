# Desktop release

`.github/workflows/release-desktop.yml` builds the desktop app for the same `v*` release tags as `release.yml`, and adds its files to the draft release that `release.yml` creates. It is a separate workflow so the CLI release, its provenance script and its tests stay unchanged.

## What it builds

| File | Platform | Built on |
|---|---|---|
| `hibiscus-desktop-aarch64-apple-darwin.dmg` | macOS, Apple Silicon | `macos-15` |
| `hibiscus-desktop-x86_64.AppImage` | Linux x86_64 | `ubuntu-22.04`, the oldest supported Ubuntu, so the AppImage runs on more distributions |
| `SHA256SUMS-desktop` | checksums of the two files above | |
| `hibiscus-desktop-provenance.sigstore.json` | signed build provenance for the two files | |

The app's version comes from the root `package.json` (`"version": "../../package.json"` in `tauri.conf.json`), which `scripts/bump-version.sh` already updates. The bundle ID is `com.rooxcode.hibiscus`.

## How a release runs

1. **verify** takes the commit that triggered the run and accepts it only if it is on `main`. A manual build-only run may also use `dev`.
2. **macos_arm64** and **linux_x64** check out that same triggering commit, without credentials, confirm it is the authorized one, run `npm ci`, and build with Tauri. Each records the SHA-256 of its file.
3. **provenance** checks both files against those digests, signs them with `actions/attest`, and writes `SHA256SUMS-desktop`.
4. **publish** waits up to 30 minutes for `release.yml` to create the draft release, then uploads the four files into it. It never creates a release and refuses one that is already published.

Only `publish` can write to the repository, and only `provenance` can sign.

To try the build without releasing, run the workflow by hand on `main` or `dev`. The two files are kept as run artifacts for 5 days and nothing is published. To rebuild a release, re-run the failed jobs of that tag's run; the workflow only ever builds the commit that triggered it.

## Opening the unsigned files

The app is not signed with an Apple Developer ID or notarised.

- **macOS:** open the `.dmg` and drag Hibiscus to Applications. On first launch, right-click the app and choose Open, then Open again.
- **Linux:** make the AppImage executable, then run it:

  ```bash
  chmod +x hibiscus-desktop-x86_64.AppImage && ./hibiscus-desktop-x86_64.AppImage
  ```

Check a download against `SHA256SUMS-desktop` with `shasum -a 256 -c SHA256SUMS-desktop --ignore-missing`.
