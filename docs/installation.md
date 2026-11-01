# Installation and releases

## Native source build

Versions 0.3.5 and later contain the native Rust runtime. Releases before 0.3.5 use the previous runtime. For a source build, install Rust/Cargo and a C linker, then run:

```sh
cargo install --path . --locked
hibiscus --version
```

The native binary needs neither Pi nor Node. It connects directly to configured provider APIs; supply an API key using the configured environment variable or fullscreen `/login`. Read [provider configuration and existing-data migration](native-runtime.md) before using a previous installation's config.

## Prebuilt installer

For a published native release, the bundled `install.sh` downloads a platform archive and verifies its SHA-256 checksum before atomic replacement. Supported targets are Linux musl and macOS on x86_64/aarch64. The installer needs curl, tar, install and shasum or sha256sum. It does not bootstrap Pi, Node or credentials.

`HIBISCUS_INSTALL_DIR` overrides `~/.local/bin`. `HIBISCUS_HOME` must be an absolute path and overrides `~/.hibiscus`. The v0.3.12 installer creates settings/cache/extensions directories without overwriting existing config or extension files. The extension API is available from v0.3.6. `HIBISCUS_VERSION` selects a release tag and `HIBISCUS_REPOSITORY` selects the release repository. `GITHUB_TOKEN` is optional for private release downloads and must never be shared in logs.

Use the installer from the versioned tag so its behavior matches the release:

```sh
curl -fsSL https://raw.githubusercontent.com/Rekabytes-Enterprise/hibiscus/v0.3.12/install.sh | HIBISCUS_VERSION=v0.3.12 sh
```

A dev push does not publish release assets. The GitHub release workflow builds all four platform archives and `SHA256SUMS`. The staged provenance workflow signs and verifies the archive identities before uploading its bundle with a new release; this support does not alter existing published releases.

## Optional provenance verification

The normal installer and `hibiscus update` verify SHA-256 checksums. They do not
verify publisher signatures or build origin. This staged rollout keeps their
current dependencies and download size unchanged.

For releases containing `hibiscus-provenance.sigstore.json`, you can explicitly
verify a downloaded archive before installing or running it. Use a trusted
source checkout, Python 3 and GitHub CLI. The verifier is tested with GitHub CLI
2.103.3; it checks the local bundle without requiring a GitHub account, login,
Pi or Node. No verifier is downloaded by normal installation.

Download the archive for your platform and its bundle into `downloads/`, then
use the tag's expected commit from an independently reviewed Git tag or GitHub
source view. Do not take the expected commit only from the unverified bundle.

```sh
python3 scripts/release_provenance.py verify \
  --archive downloads/hibiscus-aarch64-apple-darwin.tar.gz \
  --bundle downloads/hibiscus-provenance.sigstore.json \
  --tag vMAJOR.MINOR.PATCH --source-commit EXPECTED_40_HEX_TAG_COMMIT
```

For a draft rebuilt by the current protected workflow, also pass its independently
reviewed execution identity, for example `--workflow-ref refs/heads/dev
--workflow-commit EXPECTED_40_HEX_WORKFLOW_COMMIT`. The tag's actual source and
this workflow revision can differ. The policy verifies both, as well as archive
digests, certificate identity/issuer, hosted-runner identity and signed metadata.
It never executes the downloaded archive. Errors or missing provenance stop the
explicit verification command without falling back to checksums.

Releases published before the provenance rollout have no such bundle and remain
checksum-only; older tags/assets are not retroactively changed. A valid bundle establishes
origin and byte integrity, not that the code is vulnerability-free. See the
[provenance design and trust policy](development/release-provenance.md).

## Install with npx

The npm wrapper packages the same shell installer and pins the download to its package version. It requires Node.js 22.19+ and npm to run; the installed agent has no Node runtime dependency.

```sh
# Requires published npm package and matching native GitHub release assets:
npx @rekabytes/hibiscus@latest
```

The npm package is published separately from GitHub release assets; check the package version before expecting the native runtime from `@latest`. `HIBISCUS_VERSION`, `HIBISCUS_INSTALL_DIR`, `HIBISCUS_HOME`, `HIBISCUS_REPOSITORY` and `GITHUB_TOKEN` retain their shell-installer behavior. Use `--help` for installer help.

## Updates

`hibiscus update` updates Hibiscus only. Supported prebuilt installs use checksum verification and atomic replacement. Source/Cargo installs should update their checkout and rerun `cargo install --path . --locked`. Automatic update checks are cached and optional; `HIBISCUS_NO_UPDATE_CHECK=1` disables them. Updating Hibiscus does not manage another agent or provider catalog.

## Data directory

`~/.hibiscus/config.json` defines native providers/models; `auth.json` holds private stored API keys; `sessions/` holds native conversations; `archives/` retains pre-compaction history; `cache/` is disposable; `extensions/` holds user-created extension manifests and programs. Original external-agent data is not imported or deleted automatically.

## Publishing a release

Publication needs a separate explicit owner request. Version references in Cargo.toml, Cargo.lock, package.json, this guide and the installer fixture must agree. Prepare a bump with `sh scripts/bump-version.sh 0.3.12`, substituting the intended next version, then review and verify locally. A Git tag, GitHub Release and npm publication are separate events.

When explicitly authorized, tag the exact approved release commit with an annotated tag such as `v0.3.12`. The release workflow verifies the tag/version, tests, builds platform archives, verifies signed provenance and uploads assets to a draft through the isolated publisher. Publish that draft only after independent verification. Rebuilds require an existing draft; already published releases are not overwritten. The npm package must be published separately at the matching version. Never force-move an existing tag to replace a release.

Release workflows keep compilation read-only and verify compiler hashes before a separate publisher uploads archives. See [workflow security and recovery](development/release-workflows.md).
