#!/usr/bin/env bash
# The image rule at its final commit (the second, a test's `alt=""`):
# pw-core whole, `just evidence-gates` whole now that the worktree has its
# Node dependencies (installed offline from pnpm's store), and the guide's
# mutation controls whole, whose browser baseline needed them.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/img-alt
export PORT=13853
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/img-alt
LOG="$SP/img-final.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "commit $(git rev-parse HEAD); uncommitted code paths: $(git status --porcelain -- . ':(exclude)docs' | wc -l)"
step "cargo fmt --check; pw-core, every test"
cargo fmt --all --check >> "$LOG" 2>&1; echo "exit fmt $?" >> "$LOG"
cargo test --locked --no-fail-fast -p pw-core > "$SP/img-final-pw-core.log" 2>&1; echo "exit pw-core $?" >> "$LOG"
grep -E "^test result" "$SP/img-final-pw-core.log" | awk '{p+=$4; f+=$6; i+=$8} END {print "pw-core: " p " passed, " f " failed, " i " ignored"}' >> "$LOG"
grep -E "FAILED|panicked at" "$SP/img-final-pw-core.log" | head -20 >> "$LOG"
step "just evidence-gates, whole"
just evidence-gates > "$SP/img-final-evidence-gates.log" 2>&1; echo "exit evidence-gates $?" >> "$LOG"
grep -E "^Ran [0-9]+ tests|^OK|^FAILED|^# (tests|pass|fail)|^ERROR:|^FAIL:" "$SP/img-final-evidence-gates.log" >> "$LOG"
step "mutation controls: guide, whole"
CARGO_INCREMENTAL=0 python3 scripts/guide_mutations.py > "$SP/img-final-mut-guide.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "KILLED|SURVIVED|REFUSED|DID NOT BUILD|mutants killed|killed;|^FAIL|baseline" "$SP/img-final-mut-guide.log" | tail -90 >> "$LOG"
step "restored"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
git status --porcelain -- . ':(exclude)docs' >> "$LOG"
echo IMG_FINAL_FINISHED >> "$LOG"
