#!/usr/bin/env bash
# TodoMVC in Pleris (track `todomvc`, W7), built for the browser suite into
# `dist-todomvc`: its page, its handlers, its components and its plan, from
# `examples/todomvc` and the platform's packages, and TodoMVC's own
# stylesheets beside them. The development server serves it with the todos'
# data layer, chosen by what its contracts import (`todomvc:data/…`). The
# browser suite serves it on hosts of its own, one per engine
# (TODOMVC_PORTS), because a session's todos are one host's.
#
# The stylesheets are TodoMVC's, from the npm packages the owner approved
# (2026-10-08), copied as they are: `todomvc-common`'s base.css (MIT) and
# `todomvc-app-css`'s index.css (CC BY 4.0: its licence travels with it, and
# the page credits it). The third, `todomvc-pleris.css`, is this app's own,
# generated from the second by `todomvc-styles.mjs` (see it for why).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SPIKE="$REPO_ROOT/spikes/own-renderer"
OUT="${PW_OUT:-$SPIKE/dist-todomvc}"

TODOMVC=(
  "$REPO_ROOT/packages/pw-std/"*.pw
  "$REPO_ROOT/packages/pw-platform-web/"*.pw
  "$REPO_ROOT/examples/todomvc/"*.pw
)

APP_CSS="$SPIKE/node_modules/todomvc-app-css"
COMMON_CSS="$SPIKE/node_modules/todomvc-common"
for f in "$APP_CSS/index.css" "$APP_CSS/license" "$COMMON_CSS/base.css" "$COMMON_CSS/license"; do
  if [ ! -f "$f" ]; then
    echo "todomvc.sh: $f is missing; run pnpm install" >&2
    exit 1
  fi
done

rm -rf "$OUT"
mkdir -p "$OUT"

cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  check --plain "${TODOMVC[@]}"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  build --out "$OUT/build" "${TODOMVC[@]}"
cp "$SPIKE/public/pw-runtime.mjs" "$OUT/"

# TodoMVC's own stylesheets, as the packages ship them, and the notices that
# go with them; and this app's own, after them.
cp "$COMMON_CSS/base.css" "$OUT/todomvc-base.css"
cp "$APP_CSS/index.css" "$OUT/todomvc-app.css"
cp "$COMMON_CSS/license" "$OUT/todomvc-common.license.txt"
cp "$APP_CSS/license" "$OUT/todomvc-app-css.license.txt"
node "$SPIKE/todomvc-styles.mjs" "$APP_CSS/index.css" "$OUT"

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
' "${TODOMVC[@]}" > "$OUT/sources.json"
if [ "${PW_PRODUCTION_ASSETS:-0}" = 1 ]; then
  node "$SPIKE/optimize-assets.mjs" "$OUT"
fi

echo "built TodoMVC into $OUT"
