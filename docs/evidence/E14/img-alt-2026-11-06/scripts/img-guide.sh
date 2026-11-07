#!/usr/bin/env bash
# The image branch's guide controls, whole, once the browser plan's server is
# built: its first run refused its baseline because this target had no
# `debug/pw-dev-server` (exit 127 starting the web server).
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/img-alt
export PORT=13853
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/img-alt
LOG="$SP/img-guide.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "commit $(git rev-parse HEAD); uncommitted code paths: $(git status --porcelain -- . ':(exclude)docs' | wc -l)"
step "build the browser plan's server (debug)"
cargo build --locked -p pw-dev-server > "$SP/img-guide-build.log" 2>&1; echo "exit build $?" >> "$LOG"
step "mutation controls: guide, whole"
CARGO_INCREMENTAL=0 python3 scripts/guide_mutations.py > "$SP/img-guide-mut.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "KILLED|SURVIVED|REFUSED|DID NOT BUILD|mutants killed|killed;|^FAIL|baseline" "$SP/img-guide-mut.log" | tail -100 >> "$LOG"
step "restored"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
git status --porcelain -- . ':(exclude)docs' >> "$LOG"
echo IMG_GUIDE_FINISHED >> "$LOG"
