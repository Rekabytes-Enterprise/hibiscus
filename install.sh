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

# Pi's installer may have installed a standalone Node.js in an earlier run.
# Its child process cannot change this shell's PATH, so make it available here.
node_bin="${XDG_DATA_HOME:-$HOME/.local/share}/pi-node/current/bin"
if [ -x "$node_bin/node" ]; then
    PATH="$node_bin:$PATH"
    export PATH
fi

# Pi is required for every Hibiscus run. Install it only when there is no
# usable Pi already on PATH, in the destination, or in Pi's managed agent dir.
# Pi's official installer owns its Node/npm bootstrap and Pi release layout.
ensure_pi() {
    if [ -n "${HIBISCUS_PI:-}" ]; then
        command -v "$HIBISCUS_PI" >/dev/null 2>&1 || fail "HIBISCUS_PI is not an executable: $HIBISCUS_PI"
        "$HIBISCUS_PI" --version >/dev/null 2>&1 || fail "HIBISCUS_PI could not run: $HIBISCUS_PI"
        return
    fi
    if command -v pi >/dev/null 2>&1; then
        pi --version >/dev/null 2>&1 || fail "Pi exists on PATH but could not run; check Node.js or Pi installation"
        return
    fi

    managed_pi="${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}/bin/pi"
    npm_pi="$HOME/.local/bin/pi"
    if [ ! -x "$INSTALL_DIR/pi" ] && [ ! -x "$managed_pi" ] && [ ! -x "$npm_pi" ]; then
        if command -v npm >/dev/null 2>&1 && command -v node >/dev/null 2>&1 &&
            node -e 'const [major, minor] = process.versions.node.split(".").map(Number); process.exit(major > 22 || (major === 22 && minor >= 19) ? 0 : 1)' >/dev/null 2>&1; then
            printf 'Installing Pi with npm...\n'
            npm install -g --ignore-scripts --prefix "$HOME/.local" --no-fund --no-audit \
                @earendil-works/pi-coding-agent || fail "Pi npm installation failed"
        else
            # Pi's installer can bootstrap missing Node.js/npm. It may prompt
            # for permission to install Node on an interactive terminal.
            printf 'Installing Pi from pi.dev...\n'
            curl -fsSL --proto '=https' --proto-redir '=https' --connect-timeout 5 --max-time 60 \
                https://pi.dev/install.sh -o "$tmp/pi-install.sh" || fail "could not download Pi installer"
            sh -n "$tmp/pi-install.sh" || fail "invalid Pi installer"
            PATH="$INSTALL_DIR:$PATH" sh "$tmp/pi-install.sh" || fail "Pi installation failed"
            if [ -x "$node_bin/node" ]; then
                PATH="$node_bin:$PATH"
                export PATH
            fi
        fi
    fi

    # Either installer can place Pi outside the Hibiscus bin directory. Link
    # it in without replacing an existing file or changing Pi's own layout.
    if [ ! -x "$INSTALL_DIR/pi" ]; then
        pi_launcher=
        if [ -x "$managed_pi" ]; then pi_launcher=$managed_pi
        elif [ -x "$npm_pi" ]; then pi_launcher=$npm_pi
        fi
        [ -n "$pi_launcher" ] || fail "Pi installer finished but pi was not found"
        if [ -e "$INSTALL_DIR/pi" ] || [ -L "$INSTALL_DIR/pi" ]; then
            fail "cannot link Pi: $INSTALL_DIR/pi already exists"
        fi
        ln -s "$pi_launcher" "$INSTALL_DIR/pi" || fail "could not link Pi in $INSTALL_DIR"
    fi
    [ -x "$INSTALL_DIR/pi" ] || fail "Pi installer finished but pi is unavailable in $INSTALL_DIR"
    "$INSTALL_DIR/pi" --version >/dev/null 2>&1 || fail "installed Pi could not run; check Node.js and Pi installation"
}

mkdir -p "$INSTALL_DIR"
ensure_pi

# Extract only the expected file; never let paths in the archive choose where to write.
tar -xOzf "$tmp/$archive" hibiscus >"$tmp/hibiscus" || fail "release archive does not contain hibiscus"
# Never truncate a running executable. Stage beside it, then atomically replace it.
staged=$(mktemp "$INSTALL_DIR/.hibiscus-install.XXXXXXXX")
install -m 755 "$tmp/hibiscus" "$staged"
mv -f "$staged" "$INSTALL_DIR/hibiscus"

printf 'Installed hibiscus %s to %s/hibiscus\n' "$VERSION" "$INSTALL_DIR"
case ":$PATH:" in
    *":$INSTALL_DIR:"*) ;;
    *) printf 'Add %s to PATH to run hibiscus.\n' "$INSTALL_DIR" ;;
esac
printf 'Pi is ready. Sign in to a provider with /login in Hibiscus or Pi.\n'
