# Installation and releases

## Prebuilt installer

The release installer supports Linux and macOS on x86-64 and ARM64. Linux releases are statically linked musl binaries and work in WSL. Native Windows is not supported.

Review and run the installer:

```sh
curl -fsSLO https://raw.githubusercontent.com/Rekabytes-Enterprise/hibiscus/main/install.sh
less install.sh
sh install.sh
```

Or run it directly:

```sh
curl -fsSL https://raw.githubusercontent.com/Rekabytes-Enterprise/hibiscus/main/install.sh | sh
```

The default destination is `~/.local/bin/hibiscus`. The installer does not use `sudo`; set a different writable directory when needed:

```sh
HIBISCUS_INSTALL_DIR="$HOME/bin" sh install.sh
```

Install a specific release with `HIBISCUS_VERSION=v0.1.1`. For a private GitHub repository, export a token that can read repository releases, and fetch the script through GitHub's authenticated API:

```sh
export GITHUB_TOKEN=YOUR_TOKEN
curl -fsSL \
  -H "Authorization: Bearer $GITHUB_TOKEN" \
  -H "Accept: application/vnd.github.raw+json" \
  https://api.github.com/repos/Rekabytes-Enterprise/hibiscus/contents/install.sh | sh
```

Do not put the token directly in shell history or commit it to a file. The installer passes `GITHUB_TOKEN` to subsequent release downloads, detects the platform, downloads the matching archive and `SHA256SUMS`, verifies the checksum, and only then installs the executable.

Verify the result with:

```sh
hibiscus --version
```

To update a prebuilt installation later, run `hibiscus update`. This fetches the latest public GitHub release, verifies its SHA-256 checksum, and atomically replaces the installed binary without `sudo` or running a downloaded script. Restart Hibiscus afterward. The updater supports the default `~/.local/bin/hibiscus` install; if you installed elsewhere, set `HIBISCUS_INSTALL_DIR` to its directory. It will not replace a symlink or a source/Cargo build; use Cargo to update those. It needs `curl`, `tar`, `install`, and `sha256sum` or `shasum`, plus internet access. Offline failures leave the installed binary untouched. Full-screen chat checks at most once per day and offers Later / Update now; set `HIBISCUS_NO_UPDATE_CHECK=1` to opt out of automatic checks. The manual command still works. A new push alone isn't an update: publish a version-matching GitHub Release.

Prebuilt Hibiscus does not require Rust or a C compiler. Pi remains a runtime dependency and must be available as `pi` on `PATH` (or through `HIBISCUS_PI`). Node.js 22.19+ is required for inline Codex authentication; without it, Hibiscus offers Pi's authentication TUI fallback.

## Build from source

```sh
cargo test
cargo install --path .
```

Building from source requires Rust/Cargo and a C linker. On Ubuntu or WSL:

```sh
sudo apt update
sudo apt install -y build-essential
```

## Publishing a release

The release version in `Cargo.toml`, `Cargo.lock`, the docs, and Git tag must match. Before tagging, use `sh scripts/bump-version.sh 0.1.1` (substitute the intended version), review the diff, test, commit, and merge the bump into the intended release branch. The script does **not** create or move tags. Push an annotated version tag pointing at that exact release commit:

```sh
git tag -a v0.1.1 -m "Hibiscus v0.1.1"
git push origin v0.1.1
```

A previously pushed tag keeps pointing at its original commit; later changes to `dev` or `main` do not fix that tag's release run. Do not force-move a public tag without coordinating with users; a new version tag is safer.

`.github/workflows/release.yml` runs formatting, tests, and Clippy; builds these targets; generates `SHA256SUMS`; and creates the GitHub release:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

The workflow can be dispatched manually to validate all four builds without publishing a release. Release publishing requires GitHub Actions to have `contents: write` permission. macOS artifacts are ad-hoc signed, not Apple Developer ID signed or notarized.
