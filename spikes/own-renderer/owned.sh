#!/usr/bin/env bash
# The owned Notes fixture and its pages (`examples/owned`), built for the
# row-authority browser witnesses into `dist-owned`: the platform's packages
# added, its artifacts, the runtime beside them and the record of what it was
# built from (`pw build --dist`, ADR-0429), as `guestbook.sh` builds the
# guestbook. The server uses its generic declared-table adapter in memory and
# on PostgreSQL; this app has no Rust data layer of its own. Until L2
# (ADR-0395) it built the browser's WebAssembly too, which master's runtime
# no longer reads.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cargo run --quiet -p pw-cli --manifest-path "$REPO_ROOT/Cargo.toml" -- build --plain \
  --dist "${PW_OUT:-$REPO_ROOT/spikes/own-renderer/dist-owned}" "$REPO_ROOT/examples/owned"
