#!/bin/sh
set -eu

REPOSITORY=${HIBISCUS_REPOSITORY:-Rekabytes-Enterprise/hibiscus}
VERSION=${HIBISCUS_VERSION:-latest}
INSTALL_DIR=${HIBISCUS_INSTALL_DIR:-"$HOME/.local/bin"}

fail() {
    printf 'hibiscus installer: %s\n' "$*" >&2
    exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required"
command -v tar >/dev/null 2>&1 || fail "tar is required"

case "$(uname -s)" in
    Linux) os=unknown-linux-musl ;;
    Darwin) os=apple-darwin ;;
    *) fail "unsupported operating system: $(uname -s)" ;;
esac

case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    arm64 | aarch64) arch=aarch64 ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
esac

target="$arch-$os"
archive="hibiscus-$target.tar.gz"
if [ "$VERSION" = latest ]; then
    base="https://github.com/$REPOSITORY/releases/latest/download"
else
    case "$VERSION" in v*) tag=$VERSION ;; *) tag="v$VERSION" ;; esac
    base="https://github.com/$REPOSITORY/releases/download/$tag"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/hibiscus-install.XXXXXXXX")
staged=
trap 'rm -rf "$tmp"; [ -z "$staged" ] || rm -f "$staged"' EXIT HUP INT TERM

download() {
    url=$1
    destination=$2
    if [ -n "${GITHUB_TOKEN:-}" ]; then
        curl -fsSL --connect-timeout 5 --max-time 180 -H "Authorization: Bearer $GITHUB_TOKEN" "$url" -o "$destination"
    else
        curl -fsSL --connect-timeout 5 --max-time 180 "$url" -o "$destination"
    fi
}

download "$base/$archive" "$tmp/$archive" || fail "could not download $archive"
download "$base/SHA256SUMS" "$tmp/SHA256SUMS" || fail "could not download checksums"

expected=$(awk -v file="$archive" '$2 == file || $2 == "*" file { print $1; exit }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "release checksum for $archive is missing"
if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$tmp/$archive" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$tmp/$archive" | awk '{print $1}')
else
    fail "sha256sum or shasum is required to verify the download"
fi
[ "$actual" = "$expected" ] || fail "checksum verification failed"

# Extract only the expected file; never let paths in the archive choose where to write.
tar -xOzf "$tmp/$archive" hibiscus >"$tmp/hibiscus" || fail "release archive does not contain hibiscus"
mkdir -p "$INSTALL_DIR"
# Never truncate a running executable. Stage beside it, then atomically replace it.
staged=$(mktemp "$INSTALL_DIR/.hibiscus-install.XXXXXXXX")
install -m 755 "$tmp/hibiscus" "$staged"
mv -f "$staged" "$INSTALL_DIR/hibiscus"

printf 'Installed hibiscus %s to %s/hibiscus\n' "$VERSION" "$INSTALL_DIR"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) printf 'Add %s to PATH to run hibiscus.\n' "$INSTALL_DIR" ;;
esac
printf 'Pi is a runtime dependency; Node.js 22.19+ is required for inline Codex login.\n'
