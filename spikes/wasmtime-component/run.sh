#!/usr/bin/env bash
# spike: wasmtime-component — build, run, and record evidence.
# Charter §14 Milestone 0 task 10.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SPIKE="$REPO_ROOT/spikes/wasmtime-component"
EVIDENCE="$REPO_ROOT/docs/evidence/E0"
export PATH="$REPO_ROOT/.toolchain/prefix/bin:$PATH"
mkdir -p "$EVIDENCE"

OUT="$EVIDENCE/spike-wasmtime-component.txt"

MIN="$SPIKE/guest-minimal/target/wasm32-wasip2/release/spike_wasmtime_guest_minimal.wasm"
STD="$SPIKE/guest/target/wasm32-wasip2/release/spike_wasmtime_guest.wasm"

{
    echo "spike: wasmtime-component"
    echo "generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "rustc: $(rustc --version)"
    echo "wasmtime cli: $(wasmtime --version 2>/dev/null || echo '(not on PATH)')"
    echo "wasmtime crate: 48.0.8   wit-bindgen: 0.60.0"
    echo "target: wasm32-wasip2 (WASI 0.2 / Component Model)"
    echo "host: $(uname -sm)"
    echo "commands:"
    echo "  (cd guest         && cargo build --release --target wasm32-wasip2)"
    echo "  (cd guest-minimal && cargo build --release --target wasm32-wasip2)"
    echo "  (cd host          && cargo build --release)"
    echo "  host/target/release/spike-wasmtime-host <minimal.wasm> observe:<std.wasm>"
    echo

    echo "=========================== 1. BUILD ==============================="
    ( cd "$SPIKE/guest"         && cargo build --release --target wasm32-wasip2 2>&1 | tail -2 )
    ( cd "$SPIKE/guest-minimal" && cargo build --release --target wasm32-wasip2 2>&1 | tail -2 )
    ( cd "$SPIKE/host"          && cargo build --release 2>&1 | tail -2 )
    echo
    echo "artifact sizes:"
    printf '  %-16s %8s bytes  (std, wasm32-wasip2)\n'      "guest"         "$(wc -c < "$STD" | tr -d ' ')"
    printf '  %-16s %8s bytes  (no_std, wasm32-wasip2)\n'   "guest-minimal" "$(wc -c < "$MIN" | tr -d ' ')"
    echo

    echo "===================== 2. WASI VERSION ACTUALLY USED ================"
    echo "  The wasm32-wasip2 target produced imports at version:"
    ( cd "$SPIKE/host" && ./target/release/spike-wasmtime-host "observe:$STD" 2>&1 ) \
        | grep -o 'wasi:[a-z-]*/[a-z-]*@[0-9.]*' | sort -u | sed 's/^/    /'
    echo "  => charter §10.3 decision: WASI 0.2 is the stable path. See ADR-0008."
    echo

    echo "======================= 3. CAPABILITY CHECKS ======================="
    ( cd "$SPIKE/host" && ./target/release/spike-wasmtime-host "$MIN" "observe:$STD" 2>&1 )
} | tee "$OUT"

echo
echo "spike-wasmtime-component: evidence written to $OUT"
