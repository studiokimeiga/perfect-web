#!/usr/bin/env bash
# The owned Notes fixture and its pages, built for row-authority browser
# witnesses. The server uses its generic declared-table adapter in memory
# and on PostgreSQL; this app has no Rust data layer of its own.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SPIKE="$REPO_ROOT/spikes/own-renderer"
OUT="${PW_OUT:-$SPIKE/dist-owned}"

OWNED=(
  "$REPO_ROOT/packages/pw-std/"*.pw
  "$REPO_ROOT/packages/pw-platform-web/"*.pw
  "$REPO_ROOT/examples/owned/"*.pw
)

rm -rf "$OUT"
mkdir -p "$OUT"

cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  check --plain "${OWNED[@]}"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  build --out "$OUT/build" "${OWNED[@]}"
cp "$SPIKE/public/pw-runtime.mjs" "$OUT/"

# The browser's WebAssembly, as `run.sh` builds it.
cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" \
  -p pw-resume-wasm -p pw-render-wasm --target wasm32-unknown-unknown --profile browser
cp "$REPO_ROOT/target/wasm32-unknown-unknown/browser/pw_resume_wasm.wasm" "$OUT/pw-resume.wasm"
cp "$REPO_ROOT/target/wasm32-unknown-unknown/browser/pw_render_wasm.wasm" "$OUT/pw-render.wasm"

# What it was built from, each source by its digest (ADR-0166): the browser
# suite does not serve a build whose sources have changed since.
node -e '
const { createHash } = require("node:crypto");
const { readFileSync } = require("node:fs");
const files = process.argv.slice(1).map((path) => ({
  path,
  sha256: createHash("sha256").update(readFileSync(path)).digest("hex"),
}));
console.log(JSON.stringify({ files }, null, 2));
' "${OWNED[@]}" > "$OUT/sources.json"
echo "built owned notes into $OUT"
