#!/usr/bin/env bash
# The feed reference app (ADR-0218), built for the browser suite into
# `dist-feed`: its pages, its handlers, its components and its plans, from
# `examples/feed`, the platform's packages added, the runtime beside them and
# the record of what it was built from (`pw build --dist`, ADR-0429). The
# development server serves it with the feed's data layer, chosen by what its
# contracts import. The browser suite serves it on hosts of its own, one per
# engine, because a post is every reader's.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- build --plain \
  --dist "${PW_OUT:-$REPO_ROOT/spikes/own-renderer/dist-feed}" "$REPO_ROOT/examples/feed"
