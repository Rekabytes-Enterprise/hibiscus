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
    grep -Fq "\"version\": \"$old\"" package.json &&
    grep -Fq "v$old" docs/installation.md &&
    grep -Fq "run_case Darwin arm64 aarch64-apple-darwin $old" tests/install.sh || {
    echo 'Version references are out of sync; inspect Cargo.lock, package.json, docs/installation.md and tests/install.sh' >&2
    exit 1
}
grep -Fq "## [$next]" CHANGELOG.md && {
    echo "CHANGELOG.md already has a section for $next" >&2
    exit 1
}

backup=$(mktemp -d "${TMPDIR:-/tmp}/hibiscus-version.XXXXXXXX")
files='CHANGELOG.md Cargo.toml Cargo.lock package.json docs/installation.md tests/install.sh'
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

# Each PR adds its notes as changes/<name>.md, in CHANGELOG.md's "### Added"
# form, so PRs never conflict in CHANGELOG.md. They join Unreleased here,
# under matching headings after any entries already there.
fragments=$(find changes -maxdepth 1 -type f -name '*.md' ! -name README.md 2>/dev/null | LC_ALL=C sort)
if [ -n "$fragments" ]; then
    status=0
    # shellcheck disable=SC2086 # fragment names have no spaces
    awk '
        BEGIN { split("Added Changed Deprecated Removed Fixed Security", seed, " "); for (i = 1; i <= 6; i++) section(seed[i]); cur = "" }
        function section(name) { if (!(name in known)) { known[name] = 1; names[++count] = name } cur = name }
        function flush(   i, name) {
            if (pre != "") printf "\n%s", pre
            for (i = 1; i <= count; i++) {
                name = names[i]
                if (old[name] new[name] != "") printf "\n### %s\n\n%s%s", name, old[name], new[name]
            }
            print ""
        }
        FILENAME != "CHANGELOG.md" {
            if (FNR == 1) cur = ""
            if (/^### /) { section(substr($0, 5)); next }
            if (!NF) next
            if (cur == "") { bad = FILENAME; exit }
            new[cur] = new[cur] $0 "\n"
            next
        }
        /^## \[Unreleased\]$/ && !seen { print; seen = 1; inside = 1; cur = ""; next }
        inside && /^## / { flush(); inside = 0 }
        inside {
            if (/^### /) section(substr($0, 5))
            else if (NF && cur == "") pre = pre $0 "\n"
            else if (NF) old[cur] = old[cur] $0 "\n"
            next
        }
        { print }
        END {
            if (bad != "") { print bad " has an entry before its \"### Added\"-style heading" > "/dev/stderr"; exit 5 }
            if (inside) flush()
            if (!seen) exit 3
        }
    ' $fragments CHANGELOG.md > "$backup/folded.tmp" || status=$?
    case $status in
    0) cp "$backup/folded.tmp" CHANGELOG.md ;;
    3) echo 'CHANGELOG.md has no "## [Unreleased]" heading' >&2; exit 1 ;;
    *) exit 1 ;;
    esac
fi

# The Unreleased entries become this version's notes; Unreleased starts empty.
status=0
awk -v version="$next" -v date="$(date +%Y-%m-%d)" '
    /^## \[Unreleased\]$/ && !seen { print; print ""; print "## [" version "] - " date; seen = 1; inside = 1; next }
    inside && /^## / { inside = 0 }
    inside && NF { entries = 1 }
    { print }
    END { if (!seen) exit 3; if (!entries) exit 4 }
' CHANGELOG.md > "$backup/changelog.tmp" || status=$?
case $status in
0) cp "$backup/changelog.tmp" CHANGELOG.md ;;
3) echo 'CHANGELOG.md has no "## [Unreleased]" heading' >&2; exit 1 ;;
4) echo 'CHANGELOG.md has nothing under "## [Unreleased]" and changes/ has no notes; add the release notes first' >&2; exit 1 ;;
*) exit "$status" ;;
esac

# Cargo.toml has one package version; Cargo updates only the workspace entry
# in Cargo.lock, leaving third-party dependency versions pinned.
old_pattern=$(printf '%s' "$old" | sed 's/\./\\./g')
sed "s/^version = \"${old_pattern}\"$/version = \"$next\"/" Cargo.toml > "$backup/manifest"
cp "$backup/manifest" Cargo.toml
cargo update --workspace --offline --quiet
sed "s/\"version\": \"${old_pattern}\"/\"version\": \"$next\"/" package.json > "$backup/package.tmp"
cp "$backup/package.tmp" package.json
sed -e "s/v${old_pattern}/v${next}/g" \
    -e "s@sh scripts/bump-version.sh ${old_pattern}@sh scripts/bump-version.sh ${next}@g" \
    docs/installation.md > "$backup/docs.tmp"
cp "$backup/docs.tmp" docs/installation.md
sed "s/\(run_case Darwin arm64 aarch64-apple-darwin \)${old_pattern}/\1${next}/" tests/install.sh > "$backup/installer.tmp"
cp "$backup/installer.tmp" tests/install.sh

# Last, so a failed bump keeps them; their notes now live in CHANGELOG.md.
# shellcheck disable=SC2086
[ -z "$fragments" ] || rm -f $fragments

printf 'Bumped Hibiscus %s -> %s in CHANGELOG.md, Cargo.toml, Cargo.lock, package.json, docs and installer fixture.\n' "$old" "$next"
if command -v git >/dev/null 2>&1 &&
   git -C "$root" rev-parse --is-inside-work-tree >/dev/null 2>&1 &&
   git -C "$root" show-ref --verify --quiet "refs/tags/v$next"; then
    printf 'Warning: v%s already exists locally. A pushed tag keeps its old commit; do not force-move it without coordinating the release.\n' "$next" >&2
fi
printf 'Review, test, commit, and tag the exact release commit before pushing.\n'
