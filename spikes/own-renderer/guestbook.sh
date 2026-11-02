#!/usr/bin/env bash
# The guide's tutorial app (ADR-0343, a guide whose examples compile): the
# guestbook in `examples/guestbook`, which `docs/guide/tutorial.md` builds
# step by step, built for the browser suite into `dist-guestbook`. The
# program declares its data as tables, and the development server serves
# them from the build's `tables.json` with the generic memory adapter, no
# data layer of the app's own (track `data-path`, ADR-0338 D2). As
# `feed.sh` builds the feed.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SPIKE="$REPO_ROOT/spikes/own-renderer"
OUT="${PW_OUT:-$SPIKE/dist-guestbook}"

GUESTBOOK=(
  "$REPO_ROOT/packages/pw-std/"*.pw
  "$REPO_ROOT/packages/pw-platform-web/"*.pw
  "$REPO_ROOT/examples/guestbook/"*.pw
)

rm -rf "$OUT"
mkdir -p "$OUT"

cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  check --plain "${GUESTBOOK[@]}"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  build --out "$OUT/build" "${GUESTBOOK[@]}"
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
' "${GUESTBOOK[@]}" > "$OUT/sources.json"
echo "built the guestbook into $OUT"
