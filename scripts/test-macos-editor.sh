#!/bin/bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
  printf '%s\n' 'Native editor checks require the supported Apple Silicon Mac environment.' >&2
  exit 2
fi

native_target="$root/target/release"
if [[ ! -f "$native_target/libsiliq_native_bridge.dylib" ]]; then
  npm run build
  bash "$root/scripts/cargo-verified.sh" build -p siliq-native-bridge --release --locked
fi

editor_check_dir="$(mktemp -d "${TMPDIR:-/tmp}/siliq-editor-checks.XXXXXX")"
trap 'rm -rf "$editor_check_dir"' EXIT
sources=()
for source in "$root"/apps/macos/Sources/*.swift; do
  case "$(basename "$source")" in App.swift|SceneApp.swift) continue ;; esac
  sources+=("$source")
done
header="$root/crates/native-bridge/include/siliq_native.h"
xcrun swiftc -swift-version 5 -target arm64-apple-macos13.0 \
  -import-objc-header "$header" -typecheck "${sources[@]}"
xcrun swiftc -swift-version 5 -target arm64-apple-macos13.0 \
  -import-objc-header "$header" "${sources[@]}" \
  "$root/apps/macos/Tests/EditorChecks.swift" \
  -L "$native_target" -lsiliq_native_bridge \
  -Xlinker -rpath -Xlinker "$native_target" \
  -o "$editor_check_dir/editor-checks"
DYLD_LIBRARY_PATH="$native_target" "$editor_check_dir/editor-checks" "$root"
