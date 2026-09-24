#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
tmp=${TMPDIR:-/tmp}/hibiscus-installer-test-$$
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/bin" "$tmp/fixtures" "$tmp/install"

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
cp "$TEST_FIXTURES/${url##*/}" "$destination"
SCRIPT
chmod +x "$tmp/bin/uname" "$tmp/bin/curl"

run_case() {
    os=$1
    arch=$2
    target=$3
    version=$4
    archive="hibiscus-$target.tar.gz"
    rm -rf "$tmp/package" "$tmp/install"
    mkdir -p "$tmp/package" "$tmp/install"
    printf '#!/bin/sh\necho test hibiscus\n' >"$tmp/package/hibiscus"
    chmod +x "$tmp/package/hibiscus"
    tar -C "$tmp/package" -czf "$tmp/fixtures/$archive" hibiscus
    checksum=$(sha256sum "$tmp/fixtures/$archive" | awk '{print $1}')
    printf '%s  %s\n' "$checksum" "$archive" >"$tmp/fixtures/SHA256SUMS"
    : >"$tmp/curl.log"

    TEST_OS=$os TEST_ARCH=$arch TEST_FIXTURES="$tmp/fixtures" TEST_CURL_LOG="$tmp/curl.log" \
        HIBISCUS_VERSION=$version HIBISCUS_INSTALL_DIR="$tmp/install" \
        PATH="$tmp/bin:$PATH" sh "$root/install.sh" >/dev/null

    test -x "$tmp/install/hibiscus"
    "$tmp/install/hibiscus" | grep -q 'test hibiscus'
    grep -q "/$archive" "$tmp/curl.log"
    grep -q '/SHA256SUMS' "$tmp/curl.log"
}

run_case Linux x86_64 x86_64-unknown-linux-musl latest
run_case Darwin arm64 aarch64-apple-darwin 0.1.1
printf 'installer tests passed\n'
