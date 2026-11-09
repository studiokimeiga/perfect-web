#!/usr/bin/env bash
# The large list (the head-to-head's third app) in Pleris, built for the browser
# suite and the head-to-head into `dist-rows`: its page, its handlers and its
# plan, from `examples/rows` and the platform's packages. The browser suite
# serves it on hosts of its own, one per engine (ROWS_PORTS).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SPIKE="$REPO_ROOT/spikes/own-renderer"
OUT="${PW_OUT:-$SPIKE/dist-rows}"

ROWS=(
  "$REPO_ROOT/packages/pw-std/"*.pw
  "$REPO_ROOT/packages/pw-platform-web/"*.pw
  "$REPO_ROOT/examples/rows/"*.pw
)

rm -rf "$OUT"
mkdir -p "$OUT"

cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  check --plain "${ROWS[@]}"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  build --out "$OUT/build" "${ROWS[@]}"
cp "$SPIKE/public/pw-runtime.mjs" "$OUT/"

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
' "${ROWS[@]}" > "$OUT/sources.json"
if [ "${PW_PRODUCTION_ASSETS:-0}" = 1 ]; then
  node "$SPIKE/optimize-assets.mjs" "$OUT"
fi

echo "built the large list into $OUT"
