# Installation and releases

## Native source build

The native migration is source-only until a matching release is published. Existing released binaries may still use Pi. Install Rust/Cargo and a C linker, then run:

```sh
cargo install --path . --locked
hibiscus --version
```

The native binary needs neither Pi nor Node. It connects directly to configured provider APIs; supply an API key using the configured environment variable or fullscreen `/login`. Read [provider configuration and existing-data migration](native-runtime.md) before using a previous installation's config.

## Prebuilt installer

For a published native release, the bundled `install.sh` downloads a platform archive and verifies its SHA-256 checksum before atomic replacement. Supported targets are Linux musl and macOS on x86_64/aarch64. The installer needs curl, tar, install and shasum or sha256sum. It does not bootstrap Pi, Node or credentials.

`HIBISCUS_INSTALL_DIR` overrides `~/.local/bin`. `HIBISCUS_HOME` must be an absolute path and overrides `~/.hibiscus`. The installer creates settings/cache directories without overwriting existing config. `HIBISCUS_VERSION` selects a release tag and `HIBISCUS_REPOSITORY` selects the release repository. `GITHUB_TOKEN` is optional for private release downloads and must never be shared in logs.

Once the v0.3.4 release is published, it can be selected with `HIBISCUS_VERSION=v0.3.4`; the current source version alone does not establish that matching native release assets exist. Do not treat a dev push as a release.

## Install with npx

The npm wrapper packages the same shell installer and pins the download to its package version. It requires Node.js 22.19+ and npm to run; the installed agent has no Node runtime dependency.

```sh
# Requires published npm package and matching native GitHub release assets:
npx @rekabytes/hibiscus@latest
```

No npm publication or native release is implied by this local migration. `HIBISCUS_VERSION`, `HIBISCUS_INSTALL_DIR`, `HIBISCUS_HOME`, `HIBISCUS_REPOSITORY` and `GITHUB_TOKEN` retain their shell-installer behavior. Use `--help` for installer help.

## Updates

`hibiscus update` updates Hibiscus only. Supported prebuilt installs use checksum verification and atomic replacement. Source/Cargo installs should update their checkout and rerun `cargo install --path . --locked`. Automatic update checks are cached and optional; `HIBISCUS_NO_UPDATE_CHECK=1` disables them. Updating Hibiscus does not manage another agent or provider catalog.

## Data directory

`~/.hibiscus/config.json` defines native providers/models; `auth.json` holds private stored API keys; `approval.json` holds the approval preference; `sessions/` holds native conversations; `archives/` retains pre-compaction history; `cache/` is disposable. Original external-agent data is not imported or deleted automatically.

## Publishing a release

Publication needs a separate explicit owner request. Version references in Cargo.toml, Cargo.lock, package.json, this guide and the installer fixture must agree. Prepare a bump with `sh scripts/bump-version.sh 0.3.4`, substituting the intended next version, then review and verify locally. A Git tag, GitHub Release and npm publication are separate events.

When explicitly authorized, tag the exact approved release commit with an annotated tag such as `v0.3.4`. The release workflow verifies the tag/version, tests, builds platform archives and publishes checksum-verified assets. The npm package must be published separately at the matching version. Never force-move an existing tag to replace a release.
