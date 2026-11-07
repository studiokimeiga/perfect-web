#!/usr/bin/env bash
# The image rule's own checks, before its commit: the generated pages, the
# guide, the corpus and its formatting, generality, the rule's tests, and
# every mutation anchor.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/img-alt
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/img-alt
LOG="$SP/img-tests.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "fmt"
cargo fmt --all --check >> "$LOG" 2>&1; echo "exit fmt $?" >> "$LOG"
step "the guide's list of codes, regenerated"
python3 scripts/guide_diagnostics.py >> "$LOG" 2>&1; echo "exit guide_diagnostics $?" >> "$LOG"
git diff --stat -- docs/guide/diagnostics.md >> "$LOG"
python3 scripts/guide_diagnostics.py --check >> "$LOG" 2>&1; echo "exit guide_diagnostics --check $?" >> "$LOG"
python3 -m unittest scripts/tests/test_guide_diagnostics.py >> "$LOG" 2>&1; echo "exit test_guide_diagnostics $?" >> "$LOG"
step "just evidence-corpus"
just evidence-corpus >> "$LOG" 2>&1; echo "exit evidence-corpus $?" >> "$LOG"
git diff -U0 -- docs/evidence/E2D/corpus-enforcement.txt | grep '^[-+][^-+]' >> "$LOG"
step "images, generality, checking_source, corpus_history, labels, ids_and_aria"
cargo test --locked --no-fail-fast -p pw-core --test images --test generality --test checking_source --test corpus_history --test labels --test ids_and_aria -- --nocapture > "$SP/img-tests-core.log" 2>&1; echo "exit pw-core tests $?" >> "$LOG"
grep -E "^test result|FAILED|panicked at|generality-tested|untested" "$SP/img-tests-core.log" >> "$LOG"
step "pw-cli: the guide"
cargo test --locked -p pw-cli --test guide > "$SP/img-guide.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "^test result|FAILED|panicked at" "$SP/img-guide.log" >> "$LOG"
grep -A 12 "panicked at" "$SP/img-guide.log" | head -40 >> "$LOG"
step "pw-syntax: the corpus as pw fmt writes it"
cargo test --locked -p pw-syntax --test corpus_fmt > "$SP/img-fmt.log" 2>&1; echo "exit corpus_fmt $?" >> "$LOG"
grep -E "^test result|FAILED|panicked at" "$SP/img-fmt.log" >> "$LOG"
grep -A 8 "panicked at" "$SP/img-fmt.log" | head -30 >> "$LOG"
step "mutation anchors"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
echo IMG_TESTS_FINISHED >> "$LOG"
