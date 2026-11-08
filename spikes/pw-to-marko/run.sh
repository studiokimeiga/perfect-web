#!/usr/bin/env bash
# spike: pw-to-marko — charter §14 M3, the Marko rendering adapter.
#
# Generates Marko routes from `.pw` with `pw emit-marko`, builds them with the
# pinned toolchain, and measures the bytes each route ships. ADR-0017 states
# what this is and is not evidence for; the short version is that streaming and
# resumption remain MARKO's behaviours, measured through `pw`.
#
# Nothing under src/routes is authored: it is deleted and regenerated here, and
# it is gitignored so it cannot drift into being the source of truth.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
# Where cargo writes its builds: CARGO_TARGET_DIR where it is set (a
# worktree's own target), else the repository's `target/`. A relative one
# is cargo's working directory's, which is this script's at its start.
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$PWD/$TARGET_DIR" ;; esac
SPIKE="$REPO_ROOT/spikes/pw-to-marko"
EVIDENCE="$REPO_ROOT/docs/evidence/E3"
mkdir -p "$EVIDENCE"

OUT="$EVIDENCE/spike-pw-to-marko.txt"

{
    echo "spike: pw-to-marko"
    echo "generated: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "host: $(uname -sm)"
    echo "node: $(node --version)"
    echo "commands:"
    echo "  pw emit-marko examples/{hello-static,counter,streamed,store}/app.pw --out spikes/pw-to-marko/src/routes/<route>"
    echo "  pnpm --filter spike-pw-to-marko build"
    echo "  node spikes/pw-to-marko/measure.mjs"
    echo "  pnpm --filter spike-pw-to-marko exec playwright test"
    echo

    echo "===================== 1. GENERATE FROM .pw ========================="
    # Deleted first: a stale template from a previous run would be measured as
    # if it were current output. `resources.mjs` is hand-written host code and
    # lives outside routes/, so it survives.
    rm -rf "$SPIKE/src/routes"
    mkdir -p "$SPIKE/src/routes/static" "$SPIKE/src/routes/counter" \
             "$SPIKE/src/routes/streamed" "$SPIKE/src/routes/store"
    cd "$REPO_ROOT"
    # E7V: the compatibility decision, compiled to wasm so the browser runs the
    # SAME code the deployment matrix tests rather than a JavaScript port.
    cargo build --quiet -p pw-resume-wasm --target wasm32-unknown-unknown --release
    mkdir -p "$SPIKE/public"
    cp "$TARGET_DIR/wasm32-unknown-unknown/release/pw_resume_wasm.wasm" \
       "$SPIKE/public/pw-resume.wasm"
    cargo run --quiet -p pw-cli -- emit-marko examples/hello-static/app.pw \
        --out "$SPIKE/src/routes/static"
    cargo run --quiet -p pw-cli -- emit-marko examples/counter/app.pw \
        --out "$SPIKE/src/routes/counter"
    cargo run --quiet -p pw-cli -- emit-marko examples/streamed/app.pw \
        --out "$SPIKE/src/routes/streamed"
    # E4 gate 1 / E5 gate 2: one page, public store data beside private cart
    # data, with the split visible in the cache policies.
    cargo run --quiet -p pw-cli -- emit-marko examples/store/app.pw \
        --out "$SPIKE/src/routes/store"

    # @marko/run routes are `+page.marko`; the adapter emits one file per view.
    mv "$SPIKE/src/routes/static/HelloStatic.marko" "$SPIKE/src/routes/static/+page.marko"
    mv "$SPIKE/src/routes/counter/Counter.marko" "$SPIKE/src/routes/counter/+page.marko"
    mv "$SPIKE/src/routes/streamed/StreamedPage.marko" "$SPIKE/src/routes/streamed/+page.marko"
    mv "$SPIKE/src/routes/store/StorePage.marko" "$SPIKE/src/routes/store/+page.marko"
    echo
    echo "--- generated: static ---"
    cat "$SPIKE/src/routes/static/+page.marko"
    echo "--- generated: counter ---"
    cat "$SPIKE/src/routes/counter/+page.marko"
    echo "--- generated: streamed ---"
    cat "$SPIKE/src/routes/streamed/+page.marko"
    echo "--- generated: store ---"
    cat "$SPIKE/src/routes/store/+page.marko"
    echo

    echo "===================== 2. BUILD ====================================="
    cd "$SPIKE"
    pnpm build 2>&1 | sed 's/\x1b\[[0-9;]*m//g' | grep -vE '^\s*$'
    echo

    echo "===================== 3. MEASURE ==================================="
    node measure.mjs
    echo

    echo "===================== 4. BROWSERS =================================="
    echo "Charter §14 M3 task 11: Chromium, Firefox and WebKit must all pass."
    echo "Run against the BUILT output, not a dev server — a dev server measures"
    echo "Vite's behaviour rather than what a user receives."
    echo
    if pnpm exec playwright --version >/dev/null 2>&1; then
        pnpm exec playwright test --reporter=list 2>&1 | sed 's/\x1b\[[0-9;]*m//g'
    else
        echo "SKIPPED — playwright not installed. Run:"
        echo "  pnpm --filter spike-pw-to-marko exec playwright install chromium firefox webkit"
    fi
} 2>&1 | tee "$OUT"

echo
echo "evidence written to ${OUT#"$REPO_ROOT"/}"
