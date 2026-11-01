#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
tmp=$(mktemp -d "${TMPDIR:-/tmp}/hibiscus-installer-test.XXXXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
mkdir -p "$tmp/bin" "$tmp/fixtures"
cat > "$tmp/bin/uname" <<'SCRIPT'
#!/bin/sh
case "$1" in -s) echo "$TEST_OS" ;; -m) echo "$TEST_ARCH" ;; *) exit 1 ;; esac
SCRIPT
cat > "$tmp/bin/curl" <<'SCRIPT'
#!/bin/sh
while [ "$#" -gt 0 ]; do
 case "$1" in -o) destination=$2; shift 2 ;; http*) url=$1; shift ;; *) shift ;; esac
done
printf '%s\n' "$url" >> "$TEST_CURL_LOG"
[ "${TEST_DOWNLOAD_FAIL:-0}" = 0 ] || exit 22
cp "$TEST_FIXTURES/${url##*/}" "$destination"
SCRIPT
# Native installation must never invoke any external agent/runtime bootstrap.
for name in pi node npm; do
 cat > "$tmp/bin/$name" <<'SCRIPT'
#!/bin/sh
printf 'forbidden runtime bootstrap\n' >> "$TEST_FORBIDDEN"
exit 99
SCRIPT
done
chmod +x "$tmp/bin/"*
run_case() {
 os=$1; arch=$2; target=$3; version=$4; mode=$5
 home="$tmp/home-$target-$mode"; mkdir -p "$home/.hibiscus"
 printf '#!/bin/sh\necho test hibiscus\n' > "$tmp/fixtures/hibiscus"
 tar -czf "$tmp/fixtures/hibiscus-$target.tar.gz" -C "$tmp/fixtures" hibiscus
 (cd "$tmp/fixtures"; shasum -a 256 "hibiscus-$target.tar.gz") > "$tmp/fixtures/SHA256SUMS"
 if [ "$mode" = checksum ]; then printf 'invalid checksum\n' > "$tmp/fixtures/SHA256SUMS"; fi
 if [ "$mode" = existing ]; then
  printf '{"model":"keep/model"}\n' > "$home/.hibiscus/config.json"
  mkdir -p "$home/.hibiscus/extensions/keep"
  printf 'user extension source\n' > "$home/.hibiscus/extensions/keep/main.py"
  printf 'user manifest\n' > "$home/.hibiscus/extensions/keep/extension.json"
 fi
 status=0
 TEST_OS=$os TEST_ARCH=$arch TEST_FIXTURES="$tmp/fixtures" TEST_CURL_LOG="$tmp/curl.log" TEST_FORBIDDEN="$tmp/forbidden" \
 TEST_DOWNLOAD_FAIL=$([ "$mode" = download ] && echo 1 || echo 0) \
 HOME="$home" HIBISCUS_HOME="$home/.hibiscus" HIBISCUS_VERSION=$version HIBISCUS_INSTALL_DIR="$home/bin" \
 PATH="$tmp/bin:/usr/bin:/bin" sh "$root/install.sh" > "$tmp/stdout" 2> "$tmp/stderr" || status=$?
 if [ "$mode" = checksum ] || [ "$mode" = download ]; then
  [ "$status" -ne 0 ]; [ ! -e "$home/bin/hibiscus" ]; return
 fi
 [ "$status" -eq 0 ] || { cat "$tmp/stderr"; exit 1; }
 "$home/bin/hibiscus" | grep -q 'test hibiscus'
 test -d "$home/.hibiscus/cache"
 test -d "$home/.hibiscus/extensions"
 if [ "$mode" = existing ]; then
  [ "$(cat "$home/.hibiscus/config.json")" = '{"model":"keep/model"}' ]
  [ "$(cat "$home/.hibiscus/extensions/keep/main.py")" = 'user extension source' ]
  [ "$(cat "$home/.hibiscus/extensions/keep/extension.json")" = 'user manifest' ]
 else
  [ "$(cat "$home/.hibiscus/config.json")" = '{}' ]
  [ -z "$(ls -A "$home/.hibiscus/extensions")" ]
 fi
 grep -q 'Hibiscus is ready. Sign in to a provider with /login.' "$tmp/stdout"
 ! grep -qi 'Pi is ready\|Hibiscus or Pi' "$tmp/stdout"
 [ ! -e "$tmp/forbidden" ]
 ! grep -q 'pi.dev\|pi-coding-agent' "$tmp/curl.log"
}
run_case Linux x86_64 x86_64-unknown-linux-musl latest existing
run_case Linux aarch64 aarch64-unknown-linux-musl latest native
run_case Darwin x86_64 x86_64-apple-darwin latest native
run_case Darwin arm64 aarch64-apple-darwin 0.3.9 missing
run_case Darwin arm64 aarch64-apple-darwin latest checksum
run_case Linux x86_64 x86_64-unknown-linux-musl latest download
printf 'native installer tests passed\n'
