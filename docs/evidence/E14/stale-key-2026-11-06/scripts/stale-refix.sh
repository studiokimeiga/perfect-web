#!/usr/bin/env bash
# After fixing the four failures: regenerate the corpus enforcement table,
# then rerun the tests that failed, the generality figure, and the anchors.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/site
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/stale-key
LOG="$SP/stale-refix.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "fmt"
cargo fmt -p pw-core --check >> "$LOG" 2>&1; echo "exit fmt $?" >> "$LOG"
step "just evidence-corpus"
just evidence-corpus >> "$LOG" 2>&1; echo "exit evidence-corpus $?" >> "$LOG"
git diff --stat -- docs/evidence/E2D/corpus-enforcement.txt >> "$LOG"
git diff -U0 -- docs/evidence/E2D/corpus-enforcement.txt | grep '^[-+][^-+]' >> "$LOG"
step "the failed tests"
cargo test --locked -p pw-core --lib diagnostics:: > "$SP/stale-refix-lib.log" 2>&1; echo "exit lib diagnostics $?" >> "$LOG"
grep -E "^test result|FAILED|panicked" "$SP/stale-refix-lib.log" >> "$LOG"
cargo test --locked -p pw-core --test corpus_history --test checking_source --test one_checker -- --nocapture > "$SP/stale-refix-tests.log" 2>&1; echo "exit history/source/one_checker $?" >> "$LOG"
grep -E "^test result|FAILED|panicked|historical compatibility|Classified|FixtureDidNot" "$SP/stale-refix-tests.log" >> "$LOG"
step "just generality"
just generality >> "$LOG" 2>&1; echo "exit generality $?" >> "$LOG"
step "mutation anchors"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
echo STALE_REFIX_FINISHED >> "$LOG"
