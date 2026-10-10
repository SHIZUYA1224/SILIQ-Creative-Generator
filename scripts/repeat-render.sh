#!/usr/bin/env bash
# Render the same document/variant N times with the release CLI, each run in
# a fresh output directory, and compare the SHA-256 of every PNG.
#
# Usage: scripts/repeat-render.sh [N] [variant] [document]
# Requires: scripts/cargo-verified.sh build --release -p render-cli
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
n="${1:-10}"
variant="${2:-square}"
doc="${3:-$root/fixtures/public-safe/documents/basic.json}"
bin="$root/target/release/siliq-render"
work="$root/out/repeat-$variant"

[[ -x "$bin" ]] || { echo "missing $bin; build the release CLI first" >&2; exit 1; }
rm -rf "$work"
mkdir -p "$work"

for i in $(seq -w 1 "$n"); do
    mkdir "$work/run-$i"
    "$bin" render --doc "$doc" --variant "$variant" \
        --resources "$root/fixtures/public-safe/resources" \
        --out-dir "$work/run-$i" --name out.png >/dev/null
    h="$( (sha256sum "$work/run-$i/out.png" 2>/dev/null || shasum -a 256 "$work/run-$i/out.png") | awk '{print $1}')"
    echo "run $i  $h"
    echo "$h" >> "$work/hashes.txt"
done

unique="$(sort -u "$work/hashes.txt" | wc -l | tr -d ' ')"
if [[ "$unique" == "1" ]]; then
    echo "RESULT: $n/$n identical ($(head -1 "$work/hashes.txt"))"
else
    echo "RESULT: $unique different hashes in $n runs" >&2
    exit 1
fi
