#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/hibiscus-bump-test.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/scripts" "$tmp/docs" "$tmp/tests" "$tmp/src" "$tmp/bin"
cp "$root/scripts/bump-version.sh" "$tmp/scripts/bump-version.sh"
printf '{"name": "@rekabytes/hibiscus", "version": "0.1.0"}\n' > "$tmp/package.json"
printf '[package]\nname = "hibiscus"\nversion = "0.1.0"\nedition = "2021"\n' > "$tmp/Cargo.toml"
printf 'fn main() {}\n' > "$tmp/src/main.rs"
printf 'Pinned v0.1.0; tag v0.1.0\nRun sh scripts/bump-version.sh 0.1.0\n' > "$tmp/docs/installation.md"
printf 'run_case Darwin arm64 aarch64-apple-darwin 0.1.0 missing\n' > "$tmp/tests/install.sh"
printf '# Changelog\n\n## [Unreleased]\n\n## [0.1.0] - 2026-01-01\n\n- First.\n' > "$tmp/CHANGELOG.md"
(cd "$tmp" && cargo generate-lockfile --offline --quiet)

# An empty Unreleased section stops the bump before anything changes.
if sh "$tmp/scripts/bump-version.sh" v0.1.1 > /dev/null 2>&1; then
    echo 'expected empty Unreleased to be rejected' >&2
    exit 1
fi
grep -q '^version = "0.1.0"$' "$tmp/Cargo.toml"
printf '# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- A bug.\n\n## [0.1.0] - 2026-01-01\n\n- First.\n' > "$tmp/CHANGELOG.md"
cp "$tmp/CHANGELOG.md" "$tmp/changelog.before"

# If Cargo fails after editing the manifest, roll back every file.
printf '#!/bin/sh\nexit 23\n' > "$tmp/bin/cargo"
chmod +x "$tmp/bin/cargo"
if PATH="$tmp/bin:$PATH" sh "$tmp/scripts/bump-version.sh" v0.1.1 > /dev/null 2>&1; then
    echo 'expected Cargo failure' >&2
    exit 1
fi
grep -q '^version = "0.1.0"$' "$tmp/Cargo.toml"
grep -q '"version": "0.1.0"' "$tmp/package.json"
grep -q '^version = "0.1.0"$' "$tmp/Cargo.lock"
grep -q 'Pinned v0.1.0' "$tmp/docs/installation.md"
cmp -s "$tmp/CHANGELOG.md" "$tmp/changelog.before"

sh "$tmp/scripts/bump-version.sh" v0.1.1 > /dev/null
grep -q '^version = "0.1.1"$' "$tmp/Cargo.toml"
grep -q '"version": "0.1.1"' "$tmp/package.json"
grep -q '^version = "0.1.1"$' "$tmp/Cargo.lock"
grep -q 'Pinned v0.1.1; tag v0.1.1' "$tmp/docs/installation.md"
grep -q 'sh scripts/bump-version.sh 0.1.1' "$tmp/docs/installation.md"
grep -q 'aarch64-apple-darwin 0.1.1 missing' "$tmp/tests/install.sh"
printf '# Changelog\n\n## [Unreleased]\n\n## [0.1.1] - %s\n\n### Fixed\n\n- A bug.\n\n## [0.1.0] - 2026-01-01\n\n- First.\n' "$(date +%Y-%m-%d)" > "$tmp/changelog.expected"
cmp "$tmp/CHANGELOG.md" "$tmp/changelog.expected"
if sh "$tmp/scripts/bump-version.sh" 0.1.1 > /dev/null 2>&1 ||
   sh "$tmp/scripts/bump-version.sh" '1.2.3-beta' > /dev/null 2>&1; then
    echo 'expected duplicate/invalid version to be rejected' >&2
    exit 1
fi
printf 'version bump tests passed\n'
