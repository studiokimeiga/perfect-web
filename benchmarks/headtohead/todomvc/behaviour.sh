#!/usr/bin/env bash
# TodoMVC's specification on every stack the head-to-head measures, in three
# engines (behaviour.spec.mjs, the head-to-head's playwright.config.mjs), judged
# by scripts/head_to_head_behaviour.py against its written expectations: each
# stack passes every test but its known gaps, which fail in every engine. This
# exits with the judge's verdict, not Playwright's: the gaps are expected
# failures, and anything else (a new failure, a gap that passes, a test that
# did not run) fails it.
#
# It needs the Pleris host's release build (target/release/pw-dev-server) and
# TodoMVC in Pleris built (spikes/own-renderer/todomvc.sh). Environment: PORT
# (the base; its hosts take PORT + 232..234), OUT (the JSON report, default
# /tmp/h2h-behaviour.json).
set -euo pipefail
HERE="$(cd "$(dirname "$0")/.." && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"
OUT="${OUT:-/tmp/h2h-behaviour.json}"
SERVER="${CARGO_TARGET_DIR:-$REPO_ROOT/target}/release/pw-dev-server"
[ -x "$SERVER" ] || { echo "behaviour.sh: $SERVER is not built (cargo build --release -p pw-dev-server)"; exit 1; }
[ -f "$REPO_ROOT/spikes/own-renderer/dist-todomvc/sources.json" ] \
  || { echo "behaviour.sh: TodoMVC in Pleris is not built (bash spikes/own-renderer/todomvc.sh)"; exit 1; }
rm -f "$OUT"
cd "$HERE"
set +e
PLAYWRIGHT_JSON_OUTPUT_NAME="$OUT" pnpm exec playwright test --reporter=json > /dev/null 2> "$OUT.stderr"
CODE=$?
set -e
echo "playwright exited $CODE (the judge reads its report; expected gaps fail by design)"
# SUMMARY: where the judge writes the data the head-to-head's summary reads;
# CYPRESS: "stack=file ..." of the Cypress judges' reports, counted with it.
EXTRA=()
if [ -n "${SUMMARY:-}" ]; then EXTRA+=(--json "$SUMMARY"); fi
for c in ${CYPRESS:-}; do EXTRA+=(--cypress "$c"); done
python3 "$REPO_ROOT/scripts/head_to_head_behaviour.py" "$OUT" "${EXTRA[@]}"
