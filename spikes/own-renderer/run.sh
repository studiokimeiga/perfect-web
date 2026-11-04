#!/usr/bin/env bash
# E7 task 2 — the own renderer, in a browser.
#
# Charter §14 M7 gate 1: a purely static `.pw` page renders entirely through
# this implementation. No Marko, no application JavaScript, no runtime.
#
#   .pw → pw check → pw emit-template → pw-render → HTML → a browser
#
# The chain is printed rather than described, so "Marko was not involved" is
# something a reader verifies from the commands rather than from a sentence.
set -euo pipefail
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SPIKE="$REPO_ROOT/spikes/own-renderer"
OUT="${PW_OUT:-$SPIKE/dist}"
# E14's harness builds a sandbox's copy of the store (ADR-0123): its sources
# in place of `examples/`, its documents and IR outside the spike. The
# platform packages and the toolchain are the repository's.
SOURCES="${PW_SOURCES:-$REPO_ROOT/examples}"
STORE_IR="${PW_STORE_IR:-$SPIKE/store-ir.json}"
# The pages' template IR, which the render below reads. A build of another
# store (`keyed-store.sh`) writes its own, so the canonical store's stays.
TEMPLATE_IR="${PW_TEMPLATE_IR:-$SPIKE/template-ir.json}"
EVIDENCE="$REPO_ROOT/docs/evidence/E7"
PORT="${PORT:-3141}"

mkdir -p "$EVIDENCE"
rm -rf "$OUT"

# The static pages: no values, no runtime, no manifest.
PAGES=(
  "$REPO_ROOT/examples/hello-static/app.pw"
  "$REPO_ROOT/examples/render/tricky.pw"
)

# The store page: the real E4/E5 demo, rendered by the own renderer. Its
# program is the whole library, because the template IR needs the handler
# identity the resume artifacts derived and that is a whole-program answer.
#
# With the standard packages it imports, since 2026-09-25. That is the unit set
# `pw check` accepts and `just e10-component` compiles the commands from. The
# template IR (which names each handler's identity) and the compiled handlers
# (filed under that identity) are emitted from it, so the two cannot come from
# different programs.
STORE=(
  "$REPO_ROOT/packages/pw-std/"*.pw
  "$REPO_ROOT/packages/pw-platform-web/"*.pw
  "$SOURCES/domain.pw"
  "$SOURCES/lib/"*.pw
  "$SOURCES/store/"*.pw
)
# What the development server serves: the store, and the signals demo
# (ADR-0130) beside it and not in it. A benchmark sandbox's sources have no
# `demo/`, and then this is the store's alone. The store's own static render
# below reads the store's sources only.
SERVED=("${STORE[@]}")
if [ -d "$SOURCES/demo" ]; then
  SERVED+=("$SOURCES/demo/"*.pw)
fi

echo "== 1. the pages are checked before they are rendered =="
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  check --plain "${PAGES[@]}"

echo
echo "== 2. checked template IR =="
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  emit-template "${PAGES[@]}" > "$TEMPLATE_IR"
echo "   $(wc -c < "$TEMPLATE_IR" | tr -d ' ') bytes"

echo
echo "== 3. HTML, by pw-render =="
cargo run --quiet -p pw-render --manifest-path "$REPO_ROOT/Cargo.toml" --bin pw-render -- \
  --out "$OUT" < "$TEMPLATE_IR"

echo
echo "== 3b. one build: the store's handlers compiled from their bodies (E10), its plan =="
# One build (ADR-0123): the handlers, the components the server runs, their
# contracts, the graph, the speculations and each page's plan, from the
# sources just checked. First, because the store's page below is rendered at
# its signals' first values, which the plan holds (ADR-0140).
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  build --out "$OUT/build" "${SERVED[@]}"

echo
echo "== 3c. the store page, with its parts manifest and the E7V decision =="
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- \
  emit-template "${STORE[@]}" > "$STORE_IR"
# The store's static render is the canonical store's, from its values file.
# A benchmark sandbox's store is served, never rendered statically, and the
# values file does not describe it: a task that shows a new value would fail
# here for a reason that is not the task's (E14, T09).
if [ "$SOURCES" = "$REPO_ROOT/examples" ]; then
  cargo run --quiet -p pw-render --manifest-path "$REPO_ROOT/Cargo.toml" --bin pw-render -- \
    --out "$OUT" --values "$SPIKE/store-values.json" --resume "$SPIKE/store-resume.json" \
    --plan "$OUT/build/pages/store.page.StorePage~ui.json" \
    --document "StorePage(47)" --partition public --compatibility B1 \
    --identity-key "own-renderer-spike-key" \
    --runtime /pw-runtime.mjs < "$STORE_IR"
else
  echo "   a sandbox's store: served, not rendered statically"
fi
cp "$SPIKE/public/pw-runtime.mjs" "$OUT/"

echo
echo "== 3d. the browser's WebAssembly =="
# The browser's WebAssembly, built for size (`[profile.browser]`): the resume
# decision, and the renderer a signal's block is rendered again by (ADR-0130).
cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" \
  -p pw-resume-wasm -p pw-render-wasm --target wasm32-unknown-unknown --profile browser
cp "$REPO_ROOT/target/wasm32-unknown-unknown/browser/pw_resume_wasm.wasm" "$OUT/pw-resume.wasm"
cp "$REPO_ROOT/target/wasm32-unknown-unknown/browser/pw_render_wasm.wasm" "$OUT/pw-render.wasm"
echo "   $(ls "$OUT" | tr '\n' ' ')"

echo
echo "== 4. the STATIC pages ship no script =="
for page in HelloStatic Tricky; do
  if grep -ql "<script" "$OUT/$page.html"; then
    echo "FAIL: $page contains a script"
    exit 1
  fi
done
echo "   0 script tags in HelloStatic and Tricky"
if [ -f "$OUT/StorePage.html" ]; then
  echo "   StorePage carries $(grep -c "<script" "$OUT/StorePage.html") script element(s):"
  echo "     the parts manifest, and the runtime that reads it"
fi

echo
echo "== 5. determinism: the same IR renders to the same bytes =="
cargo run --quiet -p pw-render --manifest-path "$REPO_ROOT/Cargo.toml" --bin pw-render -- \
  --out "$OUT.again" < "$TEMPLATE_IR"
rm -f "$OUT.again/StorePage.html"
if ! diff "$OUT/HelloStatic.html" "$OUT.again/HelloStatic.html" > /dev/null \
   || ! diff "$OUT/Tricky.html" "$OUT.again/Tricky.html" > /dev/null; then
  echo "FAIL: two renders of one IR differ"
  exit 1
fi
rm -rf "$OUT.again"
echo "   byte-identical"

# What this build was made from, each source by its digest (ADR-0166). The
# browser suite refuses a build whose sources have changed since: on
# 2026-10-03 the suite ran on a build of an experiment, and reported a page
# the sources no longer described.
node -e '
const { createHash } = require("node:crypto");
const { readFileSync } = require("node:fs");
const files = process.argv.slice(1).map((path) => ({
  path,
  sha256: createHash("sha256").update(readFileSync(path)).digest("hex"),
}));
console.log(JSON.stringify({ files }, null, 2));
' "${SERVED[@]}" "${PAGES[@]}" > "$OUT/sources.json"

# `just e10-browser` builds through here and runs the suite itself, three
# times, so one set of build steps serves both.
# Opt-in production delivery; never rewrite the source used by compiler tests.
if [ "${PW_PRODUCTION_ASSETS:-0}" = 1 ]; then
  node "$SPIKE/optimize-assets.mjs" "$OUT"
fi

if [ "${BUILD_ONLY:-}" = 1 ]; then
  exit 0
fi

echo
echo "== 6. the browser's parsed tree =="
cd "$SPIKE"
if [ ! -d node_modules ]; then
  pnpm install --silent
fi
# `pnpm exec`, not `npx`: `npx` resolves playwright from its own cache and then
# cannot find the browsers this workspace installed, so every engine but the
# one already present reports "browser not installed" — a harness failure that
# reads exactly like a renderer failure.
cargo build --quiet --manifest-path "$REPO_ROOT/Cargo.toml" -p pw-dev-server
PORT="$PORT" pnpm exec playwright test --reporter=list 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | tail -40

{
  echo "E7-2 and E7-R — the own renderer, and the store page through it"
  echo
  echo "chain:  .pw → pw check → pw emit-template → pw-render → HTML → browser"
  echo "        Marko is not in it."
  echo
  echo "pages:  $(ls "$OUT" | tr '\n' ' ')"
  echo
  echo "STATIC ROUTES"
  echo "  HelloStatic, Tricky: 0 script tags, 0 runtime, usable with JS off"
  echo "  two renders of one IR: byte-identical"
  echo
  echo "THE STORE PAGE — E7-R's vertical slice"
  echo "  server renderer → HTML with only the required anchors"
  echo "  → parts manifest → decide() → Authorised → handler attaches"
  echo "  → click Add → command → only cart-related PartIds update"
  echo
  echo "  40 assertions, 3 engine families:"
  echo "    no Marko participates in this route"
  echo "    only dynamic regions carry identity markup"
  echo "    the page is readable with JavaScript disabled"
  echo "    a compatible manifest authorises; the handler attaches"
  echo "    an INCOMPATIBLE manifest is refused and nothing attaches"
  echo "    the menu keeps node identity across the update"
  echo "    the cart element survives its content changing, twice"
  echo "    focus survives the update"
  echo "    only the cart part id updates"
  echo "    replacing a menu node makes the identity assertion RED"
  echo "    handler bytes are still eager — the E7-L gap, recorded"
  echo
  echo "LIVE PART IDENTITY"
  echo "  IdentityDomain + InstancePath + LocalPartId."
  echo "  E6 decides who shares a domain; E7-R decides how things inside it"
  echo "  are addressed. There is no independent E7 notion of who shares."
  echo
  echo "  InstanceToken = keyed BLAKE3(identity_key,"
  echo "                    domain | parent path | each part | key)[0..96 bits]"
  echo "                  base64url, 16 characters"
  echo "  and a collision within a domain REFUSES the render — correctness"
  echo "  does not rest on probability."
  echo "  A template part DEFINITION is not a document part INSTANCE: three Add"
  echo "  buttons share data-pw=0 and have three distinct addresses."
  echo "    each loop instance has a distinct address"
  echo "    every Add button works, not only the last one"
  echo "    the instance token does not reveal the item key"
  echo "    the same document renders the same tokens twice"
  echo
  echo "NO COMPONENT REPLAY — structurally, then observed"
  echo "  the client artifact contains no template renderer, and a poisoned"
  echo "    artifact makes that check go red"
  echo "  the client never fetches the server renderer"
  echo "  a MutationObserver over the whole document sees mutations ONLY inside"
  echo "    the cart part, and a menu mutation makes that check go red"
  echo
  echo "THE REAL RESOURCE PATH — E6 and E7 composed"
  echo "  the command returns { committed: true } and no cart value"
  echo "  the browser learns the value from the RESOURCE, at a version"
  echo "    rolled-back command      → no browser update"
  echo "    CartChanged(session A)   → session B unchanged"
  echo "    duplicate event          → no second transition"
  echo "    unrelated MenuChanged    → cart unchanged"
  echo "    stale version arriving   → does not overwrite newer state"
  echo "                               (and a newer one IS applied)"
} > "$EVIDENCE/own-renderer.txt"
echo
echo "evidence written to $EVIDENCE/own-renderer.txt"
