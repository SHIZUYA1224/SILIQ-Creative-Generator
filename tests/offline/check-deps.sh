#!/usr/bin/env bash
# Offline / no-AI guard for the runtime dependency graph and the release binary.
#
# 1. The normal (runtime) dependency tree of siliq-render must not contain
#    network, async-runtime, TLS, AI/ML or model-runtime crates.
# 2. The release binary must not import socket / DNS functions.
# 3. Dynamic libraries of the release binary are listed for the record.
#
# Usage: tests/offline/check-deps.sh   (after a release build)
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
status=0

deny='^(reqwest|hyper|hyper-util|h2|h3|ureq|curl|curl-sys|isahc|surf|attohttpc|tokio|async-std|smol|mio|socket2|rustls|native-tls|openssl|openssl-sys|tungstenite|tokio-tungstenite|websocket|quinn|trust-dns-resolver|hickory-resolver|ort|onnxruntime|tract-.*|candle-.*|tch|burn|burn-.*|tensorflow|tflite|llama-.*|llm|rust-bert|tokenizers|async-openai|openai.*|anthropic.*|ollama.*|mistralrs.*|whisper-rs)$'

echo "== runtime dependency tree (siliq-render, normal edges only)"
crates="$(cargo tree -p render-cli -e normal --prefix none --format '{p}' --offline 2>/dev/null | awk '{print $1}' | sort -u)"
echo "$crates" | sed 's/^/  /'
bad="$(echo "$crates" | grep -E "$deny" || true)"
if [[ -n "$bad" ]]; then
    echo "FAIL: forbidden runtime crates: $bad"
    status=1
else
    echo "OK: no network / AI crates in the runtime graph"
fi

bin="$root/target/release/siliq-render"
if [[ ! -x "$bin" ]]; then
    echo "FAIL: $bin not built"
    exit 1
fi

echo "== imported symbols of $bin"
if command -v nm >/dev/null; then
    syms="$(nm -D --undefined-only "$bin" 2>/dev/null || nm -u "$bin")"
    net="$(echo "$syms" | grep -Ew '_?(socket|connect|bind|listen|accept|accept4|getaddrinfo|gethostbyname|gethostbyname_r|getnameinfo|res_query|sendto|recvfrom)(@.*)?' || true)"
    if [[ -n "$net" ]]; then
        echo "FAIL: network symbols imported:"
        echo "$net" | sed 's/^/  /'
        status=1
    else
        echo "OK: no socket / DNS symbols imported"
    fi
fi

echo "== dynamic libraries"
if command -v ldd >/dev/null; then
    ldd "$bin" | sed 's/^/  /'
elif command -v otool >/dev/null; then
    otool -L "$bin" | sed 's/^/  /'
fi

exit $status
