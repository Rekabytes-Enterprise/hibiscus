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

# A note in changes/ without a section heading stops the bump before anything changes.
mkdir -p "$tmp/changes"
printf '# How to add notes\n' > "$tmp/changes/README.md"
printf '%s\n' '- No heading.' > "$tmp/changes/bad.md"
if sh "$tmp/scripts/bump-version.sh" v0.1.1 > /dev/null 2>&1; then
    echo 'expected a note without a heading to be rejected' >&2
    exit 1
fi
cmp -s "$tmp/CHANGELOG.md" "$tmp/changelog.before"
grep -q '^version = "0.1.0"$' "$tmp/Cargo.toml"
rm "$tmp/changes/bad.md"
# Notes from changes/ join the Unreleased entries, in file name order.
printf '### Added\n\n- New thing.\n\n### Fixed\n\n- Another bug.\n' > "$tmp/changes/b-feature.md"
printf '### Fixed\n\n- First fix.\n' > "$tmp/changes/a-fix.md"

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
[ -f "$tmp/changes/a-fix.md" ] && [ -f "$tmp/changes/b-feature.md" ]

sh "$tmp/scripts/bump-version.sh" v0.1.1 > /dev/null
grep -q '^version = "0.1.1"$' "$tmp/Cargo.toml"
grep -q '"version": "0.1.1"' "$tmp/package.json"
grep -q '^version = "0.1.1"$' "$tmp/Cargo.lock"
grep -q 'Pinned v0.1.1; tag v0.1.1' "$tmp/docs/installation.md"
grep -q 'sh scripts/bump-version.sh 0.1.1' "$tmp/docs/installation.md"
grep -q 'aarch64-apple-darwin 0.1.1 missing' "$tmp/tests/install.sh"
printf '# Changelog\n\n## [Unreleased]\n\n## [0.1.1] - %s\n\n### Added\n\n- New thing.\n\n### Fixed\n\n- A bug.\n- First fix.\n- Another bug.\n\n## [0.1.0] - 2026-01-01\n\n- First.\n' "$(date +%Y-%m-%d)" > "$tmp/changelog.expected"
cmp "$tmp/CHANGELOG.md" "$tmp/changelog.expected"
# The notes now live in CHANGELOG.md; the README stays.
[ ! -e "$tmp/changes/a-fix.md" ] && [ ! -e "$tmp/changes/b-feature.md" ] && [ -f "$tmp/changes/README.md" ]
if sh "$tmp/scripts/bump-version.sh" 0.1.1 > /dev/null 2>&1 ||
   sh "$tmp/scripts/bump-version.sh" '1.2.3-beta' > /dev/null 2>&1; then
    echo 'expected duplicate/invalid version to be rejected' >&2
    exit 1
fi
printf 'version bump tests passed\n'
