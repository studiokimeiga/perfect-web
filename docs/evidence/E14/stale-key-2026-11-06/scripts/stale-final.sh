#!/usr/bin/env bash
# The stale-key branch's second commit (the codes page's old-number intro):
# its generated page and test, the guide's test, then `just evidence-gates`
# whole at the final commit, and the browser test that timed out under load
# in the first run, alone.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/site
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/stale-key
LOG="$SP/stale-final.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "commit $(git rev-parse HEAD); uncommitted code paths: $(git status --porcelain -- . ':(exclude)docs' | wc -l)"
step "the guide's list of codes"
python3 scripts/guide_diagnostics.py --check >> "$LOG" 2>&1; echo "exit guide_diagnostics --check $?" >> "$LOG"
python3 -m unittest scripts/tests/test_guide_diagnostics.py >> "$LOG" 2>&1; echo "exit test_guide_diagnostics $?" >> "$LOG"
step "pw-cli: the guide"
cargo test --locked -p pw-cli --test guide > "$SP/stale-final-guide.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "^test result|FAILED|panicked at" "$SP/stale-final-guide.log" >> "$LOG"
step "the browser test that timed out under load, alone"
python3 -m unittest scripts.tests.test_browser_assertion_proof -v > "$SP/stale-final-browser.log" 2>&1; echo "exit browser_assertion_proof $?" >> "$LOG"
grep -E "^Ran |^OK|^FAILED|ERROR:|FAIL:" "$SP/stale-final-browser.log" >> "$LOG"
step "just evidence-gates, whole"
just evidence-gates > "$SP/stale-final-evidence-gates.log" 2>&1; echo "exit evidence-gates $?" >> "$LOG"
grep -E "^Ran [0-9]+ tests|^OK|^FAILED|^# (tests|pass|fail)|^ERROR:|^FAIL:" "$SP/stale-final-evidence-gates.log" >> "$LOG"
echo STALE_FINAL_FINISHED >> "$LOG"
