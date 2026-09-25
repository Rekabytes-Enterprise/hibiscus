#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/hibiscus-installer-test.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/bin" "$tmp/fixtures" "$tmp/home" "$tmp/pi-agent"

cat >"$tmp/bin/uname" <<'SCRIPT'
#!/bin/sh
case "$1" in
    -s) printf '%s\n' "$TEST_OS" ;;
    -m) printf '%s\n' "$TEST_ARCH" ;;
    *) exit 1 ;;
esac
SCRIPT

cat >"$tmp/bin/curl" <<'SCRIPT'
#!/bin/sh
url=
destination=
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) destination=$2; shift 2 ;;
        http*) url=$1; shift ;;
        *) shift ;;
    esac
done
[ -n "$url" ] && [ -n "$destination" ]
printf '%s\n' "$url" >>"$TEST_CURL_LOG"
if [ "$url" = https://pi.dev/install.sh ] && [ "${TEST_PI_DOWNLOAD_FAIL:-0}" = 1 ]; then
    exit 22
fi
cp "$TEST_FIXTURES/${url##*/}" "$destination"
SCRIPT
cat >"$tmp/fixtures/install.sh" <<'SCRIPT'
#!/bin/sh
[ "${TEST_PI_INSTALL_FAIL:-0}" = 0 ] || exit 23
printf 'Pi installer invoked\n' >> "$TEST_PI_LOG"
mkdir -p "$PI_CODING_AGENT_DIR/bin"
printf '#!/bin/sh\necho pi-test-version\n' > "$PI_CODING_AGENT_DIR/bin/pi"
chmod +x "$PI_CODING_AGENT_DIR/bin/pi"
SCRIPT
cat >"$tmp/bin/npm" <<'SCRIPT'
#!/bin/sh
printf '%s\n' "$*" >> "$TEST_NPM_LOG"
[ "${TEST_NPM_INSTALL_FAIL:-0}" = 0 ] || exit 24
mkdir -p "$HOME/.local/bin"
printf '#!/bin/sh\necho npm-pi-version\n' > "$HOME/.local/bin/pi"
chmod +x "$HOME/.local/bin/pi"
SCRIPT
chmod +x "$tmp/bin/uname" "$tmp/bin/curl" "$tmp/bin/npm"

run_case() {
    os=$1
    arch=$2
    target=$3
    version=$4
    mode=$5
    archive="hibiscus-$target.tar.gz"
    rm -rf "$tmp/package" "$tmp/home/.local" "$tmp/pi-agent/bin"
    mkdir -p "$tmp/package" "$tmp/home/.local/bin"
    printf '#!/bin/sh\necho test hibiscus\n' >"$tmp/package/hibiscus"
    chmod +x "$tmp/package/hibiscus"
    tar -C "$tmp/package" -czf "$tmp/fixtures/$archive" hibiscus
    checksum=$(shasum -a 256 "$tmp/fixtures/$archive" | awk '{print $1}')
    printf '%s  %s\n' "$checksum" "$archive" >"$tmp/fixtures/SHA256SUMS"
    : >"$tmp/curl.log"
    : >"$tmp/pi.log"
    : >"$tmp/npm.log"

    rm -f "$tmp/bin/pi" "$tmp/bin/custom-pi" "$tmp/bin/node"
    if [ "$mode" = npm ] || [ "$mode" = npm_failure ]; then
        printf '#!/bin/sh\nexit 0\n' > "$tmp/bin/node"
    else
        printf '#!/bin/sh\nexit 1\n' > "$tmp/bin/node"
    fi
    chmod +x "$tmp/bin/node"
    override_pi=
    case "$mode" in
        existing)
            printf '#!/bin/sh\necho existing-pi\n' > "$tmp/bin/pi"
            chmod +x "$tmp/bin/pi" ;;
        managed)
            mkdir -p "$tmp/pi-agent/bin"
            printf '#!/bin/sh\necho managed-pi\n' > "$tmp/pi-agent/bin/pi"
            chmod +x "$tmp/pi-agent/bin/pi" ;;
        override)
            override_pi="$tmp/bin/custom-pi"
            printf '#!/bin/sh\necho custom-pi\n' > "$override_pi"
            chmod +x "$override_pi" ;;
    esac
    status=0
    TEST_OS=$os TEST_ARCH=$arch TEST_FIXTURES="$tmp/fixtures" TEST_CURL_LOG="$tmp/curl.log" \
        TEST_PI_LOG="$tmp/pi.log" TEST_NPM_LOG="$tmp/npm.log" \
        TEST_PI_DOWNLOAD_FAIL=$([ "$mode" = download_failure ] && echo 1 || echo 0) \
        TEST_PI_INSTALL_FAIL=$([ "$mode" = install_failure ] && echo 1 || echo 0) \
        TEST_NPM_INSTALL_FAIL=$([ "$mode" = npm_failure ] && echo 1 || echo 0) \
        PI_CODING_AGENT_DIR="$tmp/pi-agent" HOME="$tmp/home" HIBISCUS_PI="$override_pi" \
        HIBISCUS_VERSION=$version HIBISCUS_INSTALL_DIR="$tmp/home/.local/bin" \
        PATH="$tmp/bin:$tmp/home/.local/bin:/usr/bin:/bin" sh "$root/install.sh" >/dev/null 2>"$tmp/stderr" || status=$?

    if [ "$mode" = download_failure ] || [ "$mode" = install_failure ] || [ "$mode" = npm_failure ]; then
        [ "$status" -ne 0 ]
        [ ! -e "$tmp/home/.local/bin/hibiscus" ]
        return
    fi
    [ "$status" -eq 0 ] || { printf 'Installer failed: %s\n' "$(awk 'NR == 1 {print; exit}' "$tmp/stderr")" >&2; exit 1; }
    test -x "$tmp/home/.local/bin/hibiscus"
    "$tmp/home/.local/bin/hibiscus" | grep -q 'test hibiscus'
    grep -q "/$archive" "$tmp/curl.log"
    grep -q '/SHA256SUMS' "$tmp/curl.log"
    if [ "$mode" = npm ]; then
        grep -q -- '--ignore-scripts' "$tmp/npm.log"
        ! grep -q 'pi.dev/install.sh' "$tmp/curl.log"
        [ ! -s "$tmp/pi.log" ]
        "$tmp/home/.local/bin/pi" --version | grep -q npm-pi-version
    elif [ "$mode" = existing ] || [ "$mode" = override ] || [ "$mode" = managed ]; then
        [ ! -s "$tmp/pi.log" ]
        ! grep -q 'pi.dev/install.sh' "$tmp/curl.log"
        if [ "$mode" = managed ]; then
            [ -L "$tmp/home/.local/bin/pi" ]
        fi
    else
        grep -q 'pi.dev/install.sh' "$tmp/curl.log"
        grep -q 'Pi installer invoked' "$tmp/pi.log"
        [ -x "$tmp/home/.local/bin/pi" ]
        "$tmp/home/.local/bin/pi" --version | grep -q pi-test-version
    fi
}

run_case Linux x86_64 x86_64-unknown-linux-musl latest existing
run_case Linux aarch64 aarch64-unknown-linux-musl latest override
run_case Darwin x86_64 x86_64-apple-darwin latest managed
run_case Darwin arm64 aarch64-apple-darwin 0.2.1 missing
run_case Darwin arm64 aarch64-apple-darwin latest npm
run_case Darwin x86_64 x86_64-apple-darwin latest npm_failure
run_case Darwin x86_64 x86_64-apple-darwin latest download_failure
run_case Linux aarch64 aarch64-unknown-linux-musl latest install_failure
printf 'installer tests passed\n'
