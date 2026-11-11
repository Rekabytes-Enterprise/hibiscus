#!/bin/sh
# Fetches the agent-browser binary the app bundles (tauri.conf.json externalBin)
# for the browser tool and pane. The version is pinned and the download checked
# against its SHA-256, so every build ships the same tested binary.
set -eu

version=0.41.2
target=${TAURI_ENV_TARGET_TRIPLE:-$(rustc -vV | sed -n 's/^host: //p')}
case $target in
aarch64-apple-darwin)
    asset=agent-browser-darwin-arm64
    sum=19680f06cf87d0095c318a2e80bea6b71523d95d3b651cd18d81885cd182477a ;;
x86_64-unknown-linux-gnu)
    asset=agent-browser-linux-x64
    sum=c4351f3802ffa5254d8c69f20a52e2fcf56925c6016fcf4203db25a07d811379 ;;
*)
    echo "agent-browser is not bundled for $target" >&2
    exit 1 ;;
esac

dir=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)/src-tauri/binaries
out=$dir/agent-browser-$target
sha256() { { sha256sum "$1" 2>/dev/null || shasum -a 256 "$1"; } | cut -d ' ' -f 1; }

[ -f "$out" ] && [ "$(sha256 "$out")" = "$sum" ] && exit 0
mkdir -p "$dir"
curl -fsSL --retry 3 -o "$out.part" "https://github.com/vercel-labs/agent-browser/releases/download/v$version/$asset"
if [ "$(sha256 "$out.part")" != "$sum" ]; then
    rm -f "$out.part"
    echo "agent-browser $version download for $target does not match its checksum" >&2
    exit 1
fi
chmod 755 "$out.part"
mv "$out.part" "$out"
echo "Bundling agent-browser $version for $target"
