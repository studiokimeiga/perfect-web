#!/usr/bin/env bash
# The requires change, its second pass: regenerate the codes page and the
# enforcement table, then the tests that failed and the new ones, and the guide.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/requires
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/requires
LOG="$SP/requires-tests2.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "fmt; the codes page; the enforcement table"
cargo fmt --all >> "$LOG" 2>&1; echo "exit fmt $?" >> "$LOG"
python3 scripts/guide_diagnostics.py >> "$LOG" 2>&1; echo "exit guide_diagnostics $?" >> "$LOG"
just evidence-corpus >> "$LOG" 2>&1; echo "exit evidence-corpus $?" >> "$LOG"
git diff -U0 -- docs/evidence/E2D/corpus-enforcement.txt | grep '^[-+][^-+]' >> "$LOG"
step "the failed tests and the new ones"
T=""; for t in clause_keys clause_places clauses_read_once controls_where_they_hold device_store generic_effects handles invoked_by policy_values principal qualified_device scheduled scheduled_orders session_to_browser transition_values value_relations who_may_run component_contract generality checking_source corpus_history; do T="$T --test $t"; done
cargo test --locked --no-fail-fast -p pw-core $T > "$SP/requires-tests2-core.log" 2>&1; echo "exit tests $?" >> "$LOG"
grep -E "^test result" "$SP/requires-tests2-core.log" | awk '{p+=$4; f+=$6} END {print "tests: " p " passed, " f " failed"}' >> "$LOG"
grep -E "^test .* FAILED$" "$SP/requires-tests2-core.log" | sort -u >> "$LOG"
step "the guide"
cargo test --locked -p pw-cli --test guide > "$SP/requires-guide2.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "^test result|FAILED" "$SP/requires-guide2.log" >> "$LOG"
echo REQUIRES_TESTS2_FINISHED >> "$LOG"
