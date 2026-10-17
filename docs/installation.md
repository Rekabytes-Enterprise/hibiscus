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

The default destination is `~/.local/bin/hibiscus`. If Pi is absent, Hibiscus installs the official Pi npm package into `~/.local` when compatible Node.js/npm are already present; otherwise it fetches and runs Pi's official installer from `https://pi.dev/install.sh` so Pi can bootstrap Node.js/npm. That installer may ask for Node.js installation (and possibly request `sudo`); neither path changes Pi credentials or signs you in. Hibiscus itself installs without `sudo`. If Pi is already on `PATH`, in the installation directory, or under Pi's managed agent directory, it is reused. For a custom Pi executable, set `HIBISCUS_PI` before installing and running Hibiscus. Set a different writable Hibiscus directory when needed:

```sh
HIBISCUS_INSTALL_DIR="$HOME/bin" sh install.sh
```

Once the v0.3.3 release is published, install it specifically with `HIBISCUS_VERSION=v0.3.3`. A `dev` push alone does not publish release assets. For a private GitHub repository, export a token that can read repository releases, and fetch the script through GitHub's authenticated API:

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

To update a prebuilt installation and Pi together, run `hibiscus update`. For Hibiscus, this fetches the latest public GitHub release, verifies its SHA-256 checksum, and atomically replaces the installed binary without `sudo` or running a downloaded script. Restart Hibiscus afterward. The updater supports the default `~/.local/bin/hibiscus` install; if you installed elsewhere, set `HIBISCUS_INSTALL_DIR` to its directory. It will not replace a symlink or a source/Cargo build; use Cargo to update those. It needs `curl`, `tar`, `install`, and `sha256sum` or `shasum`, plus internet access. The manual command also runs `pi update --self` (or the executable selected by `HIBISCUS_PI`), letting Pi manage its own installation. This updates Pi itself, not its extension packages or model catalogs. Pi is attempted even when Hibiscus is current or its update fails. Each component reports its outcome; any failure returns a nonzero exit status, and successful changes are not rolled back. Pi installations managed externally may require their package manager, as reported by Pi. Unsupported Hibiscus install locations are rejected before either update starts. Full-screen chat checks at most once per day and offers Later / Update now for Hibiscus only (the running Pi is not updated during chat); set `HIBISCUS_NO_UPDATE_CHECK=1` to opt out of automatic checks. The manual command still works. A new push alone isn't an update: publish a version-matching GitHub Release.

Prebuilt Hibiscus does not require Rust or a C compiler. Pi is a runtime dependency; the prebuilt installer installs it when missing and links Pi's managed launcher alongside Hibiscus when needed. Pi requires Node.js 22.19+; when Node.js/npm are absent or too old, Pi's installer may prompt to bootstrap them. If the official Pi installer fails or a non-interactive shell cannot supply missing Node, installation stops instead of claiming Hibiscus is ready. Hibiscus can find Pi's standalone Node.js even if the calling shell has not loaded its new PATH. Source/Cargo installs still require you to install Pi separately. `HIBISCUS_PI` can select a custom existing Pi.

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

The release version in `Cargo.toml`, `Cargo.lock`, the docs, and Git tag must match. Before tagging, use `sh scripts/bump-version.sh 0.3.3` (substitute the intended version), review the diff, test, commit, and merge the bump into the intended release branch. The script does **not** create or move tags. Push an annotated version tag pointing at that exact release commit:

```sh
git tag -a v0.3.3 -m "Hibiscus v0.3.3"
git push origin v0.3.3
```

A previously pushed tag keeps pointing at its original commit; later changes to `dev` or `main` do not fix that tag's release run. Do not force-move a public tag without coordinating with users; a new version tag is safer.

`.github/workflows/release.yml` runs formatting, tests, and Clippy; builds these targets; generates `SHA256SUMS`; and creates the GitHub release:

- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`

The workflow can be dispatched manually to validate all four builds without publishing a release. Release publishing requires GitHub Actions to have `contents: write` permission. macOS artifacts are ad-hoc signed, not Apple Developer ID signed or notarized.

## Hibiscus data directory

The production installer creates `~/.hibiscus/` and `~/.hibiscus/cache/` without overwriting existing configuration. Set `HIBISCUS_HOME` to an absolute path to use another location; use the same value when installing and running Hibiscus. This directory belongs to the user, not the project or filesystem root.

- `config.json` stores Hibiscus runtime options. The installer creates it with `{"codexContextWindow":1000000}`; existing files are preserved.
- `approval.json` stores your approval choice and is created when you change the setting or migrate an existing preference.
- `cache/update-check` records automatic update checks when they run.

Existing approval settings from `$XDG_CONFIG_HOME/hibiscus/approval.json` or `~/.config/hibiscus/approval.json` are copied on first launch if the new file is absent. The old file is retained; an existing new file always takes precedence. The old update cache is disposable and is not migrated. Pi's credentials, sessions and configuration remain managed by Pi in its own agent directory. Binaries stay in the installation directory; transient bundled extensions are staged in the OS temporary directory and removed on exit.

### Codex context window

Hibiscus defaults `openai-codex` models `gpt-5.4`, `gpt-5.5`, `gpt-5.6`, `gpt-5.6-sol`, `gpt-5.6-terra`, `gpt-5.6-luna`, `gpt-6-sol`, `gpt-6.1-sol`, `gpt-6-astra`, and `gpt-6-luna` to **1,000,000 tokens**. Only exact IDs already available in Pi are adjusted; older models, unknown variants, and other providers keep Pi's metadata. The allowlist follows the [OpenAI model documentation](https://developers.openai.com/api/docs/models/compare). This config changes the SDK budget, not the provider's actual capability or account access.

Edit `~/.hibiscus/config.json` (or `$HIBISCUS_HOME/config.json`) and restart Hibiscus:

```json
{"codexContextWindow": 1000000}
```

Set the value to `null` to use Pi's defaults, or an integer from 200000 through 1050000 to choose a budget for the supported models. Missing files/fields default to 1000000; malformed configuration fails at startup. Source installations can create this file manually. Pi's explicit `models.json` per-model overrides retain precedence. The bundled `model-context.mjs` updates Pi's registry and reselects the active model, so SDK compaction and RPC context usage use the same metadata. Standalone Pi is unchanged. Pi still reserves space for output/compaction; 1M is the total context window, not a promise of 1M input tokens.
