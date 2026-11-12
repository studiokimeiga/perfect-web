#!/usr/bin/env bash
# The large list (the head-to-head's third app) in Pleris, built for the browser
# suite and the head-to-head into `dist-rows`: its page, its handlers and its
# plan, from `examples/rows/app.pw`, the platform's packages added, the runtime
# beside them and the record of what it was built from (`pw build --dist`,
# ADR-0429), packed for production where PW_PRODUCTION_ASSETS is 1, keeping
# what its page reports where PW_REPORT is 1 too (ADR-0430). The browser suite
# serves it on hosts of its own, one per engine (ROWS_PORTS).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- build --plain \
  --dist "${PW_OUT:-$REPO_ROOT/spikes/own-renderer/dist-rows}" "$REPO_ROOT/examples/rows/${ROWS_PAGE:-app}.pw" \
  $([ "${PW_PRODUCTION_ASSETS:-0}" = 1 ] && echo --production) $([ "${PW_REPORT:-0}" = 1 ] && echo --report)
