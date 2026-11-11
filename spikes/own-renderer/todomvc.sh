#!/usr/bin/env bash
# TodoMVC in Pleris (track `todomvc`, W7), built for the browser suite into
# `dist-todomvc`: its page, its handlers, its components and its plan, from
# `examples/todomvc` and the platform's packages, the runtime beside them, and
# TodoMVC's own stylesheets as assets (`pw build --dist`, ADR-0429), packed
# for production where PW_PRODUCTION_ASSETS is 1. The development server serves
# it with the todos' data layer, chosen by what its contracts import
# (`todomvc:data/…`). The browser suite serves it on hosts of its own, one per
# engine (TODOMVC_PORTS), because a session's todos are one host's.
#
# The stylesheets are TodoMVC's, from the npm packages the owner approved
# (2026-10-08), copied as they are: `todomvc-common`'s base.css (MIT) and
# `todomvc-app-css`'s index.css (CC BY 4.0: its licence travels with it, and
# the page credits it). A missing one is named by `pw build`: run pnpm install.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
SPIKE="$REPO_ROOT/spikes/own-renderer"
CSS="$SPIKE/node_modules"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- build --plain \
  --dist "${PW_OUT:-$SPIKE/dist-todomvc}" "$REPO_ROOT/examples/todomvc" \
  --asset "$CSS/todomvc-common/base.css=todomvc-base.css" --asset "$CSS/todomvc-app-css/index.css=todomvc-app.css" \
  --asset "$CSS/todomvc-common/license=todomvc-common.license.txt" \
  --asset "$CSS/todomvc-app-css/license=todomvc-app-css.license.txt" \
  $([ "${PW_PRODUCTION_ASSETS:-0}" = 1 ] && echo --production)
