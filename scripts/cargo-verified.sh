#!/usr/bin/env bash
# Run cargo with a Skia binary archive whose SHA-256 has been verified against
# third_party/skia-binaries.lock. rust-skia's own build script downloads the
# archive without any integrity check, so we fetch it ourselves (development
# time only), verify it, and hand it to skia-bindings via a file:// URL.
#
# Usage: scripts/cargo-verified.sh <cargo args...>
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lock="$root/third_party/skia-binaries.lock"
cache="$root/.skia-cache"

host="$(rustc -vV | sed -n 's/^host: //p')"
read -r _ tag key sha < <(grep -v '^#' "$lock" | awk -v t="$host" '$1 == t') || {
    echo "cargo-verified: no pinned Skia binaries for host $host in $lock" >&2
    exit 1
}

archive="$cache/skia-binaries-$key.tar.gz"
url="https://github.com/rust-skia/skia-binaries/releases/download/$tag/skia-binaries-$key.tar.gz"

verify() {
    local actual
    actual="$( (sha256sum "$1" 2>/dev/null || shasum -a 256 "$1") | awk '{print $1}')"
    [[ "$actual" == "$sha" ]]
}

if [[ ! -f "$archive" ]] || ! verify "$archive"; then
    mkdir -p "$cache"
    tmp="$archive.part"
    echo "cargo-verified: downloading $url" >&2
    curl -fL --proto '=https' -o "$tmp" "$url"
    if ! verify "$tmp"; then
        rm -f "$tmp"
        echo "cargo-verified: SHA-256 mismatch for $url (expected $sha)" >&2
        exit 1
    fi
    mv "$tmp" "$archive"
fi
echo "cargo-verified: Skia binaries $key verified (sha256 $sha)" >&2

export SKIA_BINARIES_URL="file://$cache/skia-binaries-{key}.tar.gz"
exec cargo "$@"
