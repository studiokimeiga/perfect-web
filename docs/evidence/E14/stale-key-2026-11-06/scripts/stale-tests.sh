#!/usr/bin/env bash
# The stale-key change's own checks, before its commit: the compiler's tests
# whole, the guide's examples, the corpus's formatting, the registry's
# generated page, and every mutation anchor.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/site
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/stale-key
LOG="$SP/stale-tests.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "pw-core, every test"
cargo test --locked --no-fail-fast -p pw-core > "$SP/stale-pw-core.log" 2>&1; echo "exit pw-core $?" >> "$LOG"
grep -E "^test result|FAILED|panicked at|^---- " "$SP/stale-pw-core.log" | sort | uniq -c | sort -rn | head -40 >> "$LOG"
step "pw-cli: the guide"
cargo test --locked -p pw-cli --test guide > "$SP/stale-guide.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "^test result|FAILED|panicked at" "$SP/stale-guide.log" >> "$LOG"
step "pw-syntax: the corpus as pw fmt writes it"
cargo test --locked -p pw-syntax --test corpus_fmt > "$SP/stale-fmt.log" 2>&1; echo "exit corpus_fmt $?" >> "$LOG"
grep -E "^test result|FAILED|panicked at" "$SP/stale-fmt.log" >> "$LOG"
step "the guide's list of codes"
python3 scripts/guide_diagnostics.py --check >> "$LOG" 2>&1; echo "exit guide_diagnostics $?" >> "$LOG"
python3 -m unittest scripts/tests/test_guide_diagnostics.py >> "$LOG" 2>&1; echo "exit test_guide_diagnostics $?" >> "$LOG"
step "mutation anchors"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
echo STALE_TESTS_FINISHED >> "$LOG"
