#!/usr/bin/env bash
# A production build of one Pleris program (track `deploy`, ADR-XXXX, a
# production host): what a production host serves, laid out as `run.sh`
# lays a development build out.
#
#   deploy/build-dist.sh <program> <out>      program: store | feed | kiokun
#
#   <out>/build/           `pw build`'s output for the program alone (no demo
#                          pages beside it, no static renders)
#   <out>/pw-runtime.mjs   the runtime, minified (esbuild)
#   <out>/pw-resume.wasm   the browser's WebAssembly, built for size
#   <out>/pw-render.wasm   (`[profile.browser]`)
#   <out>/build-info.json  what it was built from: the commit, each source's
#                          digest, and the runtime's sizes
#
# Its tools, each named by the environment where the image's build has its
# own (deploy/Containerfile); built here otherwise:
#   PW        the compiler                (target/release/pw)
#   ESBUILD   the minifier                (spikes/own-renderer/node_modules/.bin/esbuild)
#   WASM_DIR  pw_resume_wasm.wasm and pw_render_wasm.wasm
#                                         (target/wasm32-unknown-unknown/browser)
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROGRAM="${1:?the program: store, feed or kiokun}"
OUT="${2:?the directory the build is written to}"

case "$PROGRAM" in
  store)  OWN=("$ROOT/examples/domain.pw" "$ROOT/examples/lib/"*.pw "$ROOT/examples/store/"*.pw) ;;
  feed)   OWN=("$ROOT/examples/feed/"*.pw) ;;
  kiokun) OWN=("$ROOT/examples/kiokun-site/"*.pw "$ROOT/examples/kiokun/Shards.pw"
               "$ROOT/examples/kiokun/dictionary.pw") ;;
  *) echo "deploy/build-dist.sh: no program named \`$PROGRAM\`: store, feed or kiokun" >&2; exit 2 ;;
esac
SOURCES=("$ROOT/packages/pw-std/"*.pw "$ROOT/packages/pw-platform-web/"*.pw "${OWN[@]}")

if [ -z "${PW:-}" ]; then
  cargo build --quiet --locked --release --manifest-path "$ROOT/Cargo.toml" -p pw-cli
  PW="$ROOT/target/release/pw"
fi
if [ -z "${WASM_DIR:-}" ]; then
  cargo build --quiet --locked --manifest-path "$ROOT/Cargo.toml" \
    -p pw-resume-wasm -p pw-render-wasm --target wasm32-unknown-unknown --profile browser
  WASM_DIR="$ROOT/target/wasm32-unknown-unknown/browser"
fi
ESBUILD="${ESBUILD:-$ROOT/spikes/own-renderer/node_modules/.bin/esbuild}"
if [ ! -x "$ESBUILD" ]; then
  echo "deploy/build-dist.sh: no minifier at $ESBUILD: run \`pnpm install --offline --frozen-lockfile\`" >&2
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT"
"$PW" check --plain "${SOURCES[@]}"
"$PW" build --out "$OUT/build" "${SOURCES[@]}"
cp "$WASM_DIR/pw_resume_wasm.wasm" "$OUT/pw-resume.wasm"
cp "$WASM_DIR/pw_render_wasm.wasm" "$OUT/pw-render.wasm"

# The runtime, minified: whitespace and comments gone, local names
# shortened, nothing lowered (esbuild's default target is esnext), its
# characters kept as UTF-8, which it is served as.
RUNTIME="$ROOT/spikes/own-renderer/public/pw-runtime.mjs"
"$ESBUILD" "$RUNTIME" --minify --format=esm --charset=utf8 --legal-comments=none \
  --log-level=warning --outfile="$OUT/pw-runtime.mjs"
# A runtime that does not parse is no build.
node --check "$OUT/pw-runtime.mjs"

# What it was built from: the commit (and whether the tree was clean), each
# source by its SHA-256 (as `run.sh`'s sources.json), and the runtime's
# sizes, as written and as served. An image's build has no history, and
# says its commit (PW_COMMIT, the Containerfile's COMMIT).
if [ -n "${PW_COMMIT:-}" ]; then
  COMMIT="$PW_COMMIT"
else
  COMMIT="$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)"
  if [ "$COMMIT" != unknown ] && ! git -C "$ROOT" diff --quiet HEAD -- . 2>/dev/null; then
    COMMIT="$COMMIT + uncommitted changes"
  fi
fi
node -e '
const { createHash } = require("node:crypto");
const { readFileSync } = require("node:fs");
const { gzipSync } = require("node:zlib");
const [program, commit, runtime, minified, ...sources] = process.argv.slice(1);
const size = (path) => {
  const bytes = readFileSync(path);
  return { bytes: bytes.length, gzip: gzipSync(bytes, { level: 9 }).length };
};
console.log(JSON.stringify({
  program,
  commit,
  runtime: { written: size(runtime), served: size(minified) },
  sources: sources.map((path) => ({
    path: path.slice(path.indexOf("/examples/") >= 0 ? path.indexOf("/examples/") + 1 : path.indexOf("/packages/") + 1),
    sha256: createHash("sha256").update(readFileSync(path)).digest("hex"),
  })),
}, null, 2));
' "$PROGRAM" "$COMMIT" "$RUNTIME" "$OUT/pw-runtime.mjs" "${SOURCES[@]}" > "$OUT/build-info.json"

echo "deploy/build-dist.sh: $PROGRAM built into $OUT ($(cat "$OUT/build/build-id"))"
