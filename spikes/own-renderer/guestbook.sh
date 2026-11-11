#!/usr/bin/env bash
# The guide's tutorial app (ADR-0343, a guide whose examples compile): the
# guestbook in `examples/guestbook`, which `docs/guide/tutorial.md` builds
# step by step, built for the browser suite into `dist-guestbook`, the
# platform's packages added, the runtime beside it and the record of what it
# was built from (`pw build --dist`, ADR-0429). The program declares its data
# as tables, and the development server serves them from the build's
# `tables.json` with the generic memory adapter, no data layer of the app's
# own (track `data-path`, ADR-0338 D2). As `feed.sh` builds the feed.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- build --plain \
  --dist "${PW_OUT:-$REPO_ROOT/spikes/own-renderer/dist-guestbook}" "$REPO_ROOT/examples/guestbook"
