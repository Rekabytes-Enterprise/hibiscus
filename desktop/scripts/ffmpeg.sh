#!/bin/sh
# Fetches the ffmpeg binary the app bundles (tauri.conf.json externalBin) for the
# analyze_video tool. Static GPL builds from ffmpeg-static, pinned and checked
# against the release's SHA-256, so every build ships the same tested binary.
set -eu

version=b6.1.1
target=${TAURI_ENV_TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}
case $target in
aarch64-apple-darwin)
    asset=ffmpeg-darwin-arm64.gz
    sum=8923876afa8db5585022d7860ec7e589af192f441c56793971276d450ed3bbfa ;;
x86_64-apple-darwin)
    asset=ffmpeg-darwin-x64.gz
    sum=929b375c1182d956c51f7ac25e0b2b0411fb01f6f407aa15c9758efeb4242106 ;;
x86_64-unknown-linux-gnu)
    asset=ffmpeg-linux-x64.gz
    sum=bfe8a8fc511530457b528c48d77b5737527b504a3797a9bc4866aeca69c2dffa ;;
x86_64-pc-windows-msvc)
    asset=ffmpeg-win32-x64.gz
    sum=8883a3dffbd0a16cf4ef95206ea05283f78908dbfb118f73c83f4951dcc06d77
    # Tauri's externalBin adds .exe on Windows.
    ext=.exe ;;
*)
    echo "ffmpeg is not bundled for $target" >&2
    exit 1 ;;
esac

dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)/src-tauri/binaries
out=$dir/ffmpeg-$target${ext:-}
stamp=$out.sha256
sha256() { { sha256sum "$1" 2>/dev/null || shasum -a 256 "$1"; } | cut -d ' ' -f 1; }

# The stamp records which download the unpacked binary came from.
[ -f "$out" ] && [ -f "$stamp" ] && [ "$(cat "$stamp")" = "$sum" ] && exit 0
mkdir -p "$dir"
curl -fsSL --retry 3 -o "$out.gz.part" "https://github.com/eugeneware/ffmpeg-static/releases/download/$version/$asset"
if [ "$(sha256 "$out.gz.part")" != "$sum" ]; then
    rm -f "$out.gz.part"
    echo "ffmpeg $version download for $target does not match its checksum" >&2
    exit 1
fi
gzip -dc "$out.gz.part" > "$out.part"
rm -f "$out.gz.part"
chmod 755 "$out.part"
mv "$out.part" "$out"
printf '%s' "$sum" > "$stamp"
echo "Bundling ffmpeg $version for $target"
