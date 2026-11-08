#!/usr/bin/env bash
# kiokun.com in Pleris (track `kiokun`, W6), built for the browser suite into
# `dist-kiokun`: its pages, its components and its plans, from
# `examples/kiokun-site`, kiokun's shard rule (`examples/kiokun/Shards.pw`,
# ADR-0041) and the platform's packages. The development server serves it
# with the kiokun layer, chosen by what its contracts import, from
# KIOKUN_DATA or the repository's sample. The browser suite serves it on
# hosts of its own, one per engine (KIOKUN_PORTS).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
# Where cargo writes its builds: CARGO_TARGET_DIR where it is set (a
# worktree's own target), else the repository's `target/`. A relative one
# is cargo's working directory's, which is this script's at its start.
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$PWD/$TARGET_DIR" ;; esac
SPIKE="$REPO_ROOT/spikes/own-renderer"
OUT="${PW_OUT:-$SPIKE/dist-kiokun}"

KIOKUN=(
  "$REPO_ROOT/packages/pw-std/"*.pw
  "$REPO_ROOT/packages/pw-platform-web/"*.pw
  "$REPO_ROOT/examples/kiokun-site/"*.pw
  "$REPO_ROOT/examples/kiokun/Shards.pw"
  "$REPO_ROOT/examples/kiokun/dictionary.pw"
)

rm -rf "$OUT"
mkdir -p "$OUT"

cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  check --plain "${KIOKUN[@]}"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  build --out "$OUT/build" "${KIOKUN[@]}"
cp "$SPIKE/public/pw-runtime.mjs" "$OUT/"

# The browser's WebAssembly, as `run.sh` builds it.
cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" \
  -p pw-resume-wasm -p pw-render-wasm --target wasm32-unknown-unknown --profile browser
cp "$TARGET_DIR/wasm32-unknown-unknown/browser/pw_resume_wasm.wasm" "$OUT/pw-resume.wasm"
cp "$TARGET_DIR/wasm32-unknown-unknown/browser/pw_render_wasm.wasm" "$OUT/pw-render.wasm"

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
' "${KIOKUN[@]}" > "$OUT/sources.json"
echo "built kiokun into $OUT"
