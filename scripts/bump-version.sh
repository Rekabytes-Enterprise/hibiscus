#!/bin/sh
# Prepare a new release commit. Does not create, move, or push Git tags.
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
[ "$#" -eq 1 ] || { echo 'Usage: scripts/bump-version.sh [v]MAJOR.MINOR.PATCH' >&2; exit 2; }
case $1 in v*) next=${1#v} ;; *) next=$1 ;; esac
printf '%s\n' "$next" | LC_ALL=C grep -Eq '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$' || {
    echo 'Version must be MAJOR.MINOR.PATCH (no prerelease or leading zeroes)' >&2
    exit 2
}
cd "$root"
old=$(awk -F ' *= *' '$1 == "version" { gsub(/"/, "", $2); print $2; exit }' Cargo.toml)
[ -n "$old" ] && [ "$old" != "$next" ] || {
    echo "Version must differ from current Cargo.toml version ($old)" >&2
    exit 2
}
# Don't silently skip a stale lockfile or stale release documentation.
grep -Fq "version = \"$old\"" Cargo.lock &&
    grep -Fq "v$old" docs/installation.md &&
    grep -Fq "run_case Darwin arm64 aarch64-apple-darwin $old" tests/install.sh || {
    echo 'Version references are out of sync; inspect Cargo.lock, docs/installation.md and tests/install.sh' >&2
    exit 1
}

backup=$(mktemp -d "${TMPDIR:-/tmp}/hibiscus-version.XXXXXXXX")
files='Cargo.toml Cargo.lock docs/installation.md tests/install.sh'
for file in $files; do
    mkdir -p "$backup/$(dirname "$file")"
    cp -p "$file" "$backup/$file"
done
cleanup() {
    result=$?
    if [ "$result" -ne 0 ]; then
        for file in $files; do cp -p "$backup/$file" "$file"; done
    fi
    rm -rf "$backup"
    exit "$result"
}
trap cleanup EXIT

# Cargo.toml has one package version; Cargo updates only the workspace entry
# in Cargo.lock, leaving third-party dependency versions pinned.
old_pattern=$(printf '%s' "$old" | sed 's/\./\\./g')
sed "s/^version = \"${old_pattern}\"$/version = \"$next\"/" Cargo.toml > "$backup/manifest"
cp "$backup/manifest" Cargo.toml
cargo update --workspace --offline --quiet
sed -e "s/v${old_pattern}/v${next}/g" \
    -e "s@sh scripts/bump-version.sh ${old_pattern}@sh scripts/bump-version.sh ${next}@g" \
    docs/installation.md > "$backup/docs.tmp"
cp "$backup/docs.tmp" docs/installation.md
sed "s/\(run_case Darwin arm64 aarch64-apple-darwin \)${old_pattern}/\1${next}/" tests/install.sh > "$backup/installer.tmp"
cp "$backup/installer.tmp" tests/install.sh

printf 'Bumped Hibiscus %s -> %s in Cargo.toml, Cargo.lock, docs and installer fixture.\n' "$old" "$next"
if command -v git >/dev/null 2>&1 &&
   git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1 &&
   git -C "$root" show-ref --verify --quiet "refs/tags/v$next"; then
    printf 'Warning: v%s already exists locally. A pushed tag keeps its old commit; do not force-move it without coordinating the release.\n' "$next" >&2
fi
printf 'Review, test, commit, and tag the exact release commit before pushing.\n'
