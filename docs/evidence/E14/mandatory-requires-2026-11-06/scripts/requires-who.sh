#!/usr/bin/env bash
# The requires change's own controls, whole, at its commit: the run at
# 815ec259 was stopped mid-script when the PC's disk filled (and left a
# mutant applied, since restored). Everything else at 815ec259 passed.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/requires
export CARGO_INCREMENTAL=0
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/2fa9113b97a63953/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/requires
LOG="$SP/requires-who.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "commit $(git rev-parse HEAD); uncommitted code paths: $(git status --porcelain -- . ':(exclude)docs' | wc -l)"
step "mutation controls: who_may_run, whole"
python3 scripts/who_may_run_mutations.py > "$SP/requires-mut-who_may_run.log" 2>&1; echo "exit who_may_run $?" >> "$LOG"
cat "$SP/requires-mut-who_may_run.log" >> "$LOG"
step "restored"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
git status --porcelain -- . ':(exclude)docs' >> "$LOG"
echo REQUIRES_WHO_FINISHED >> "$LOG"
