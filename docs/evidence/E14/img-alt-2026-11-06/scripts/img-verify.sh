#!/usr/bin/env bash
# The image rule at its exact commit: `just preflight`, `just
# evidence-gates`, the compiler's tests whole, the guide's test, the corpus's
# formatting, and every mutation script the change reaches or changes
# (`reached.py` against master, plus `images` and `guide`), each run whole.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/img-alt
# The guide's browser controls serve the tutorial's app on PORT; this run's
# own, away from the base default 3141 other sessions use.
export PORT=13853
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/img-alt
LOG="$SP/img-verify.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "commit $(git rev-parse HEAD); uncommitted code paths: $(git status --porcelain -- . ':(exclude)docs' | wc -l)"
echo "rust: $(rustc --version); node: $(node --version)" >> "$LOG"
step "reached mutation scripts (reached.py against master, 3 lines)"
python3 "$SP/reached.py" 5d4d4d55a36f52ea556aa8f2af3e70b1150ffcf6 3 > "$SP/img-reached.txt" 2>&1
cat "$SP/img-reached.txt" >> "$LOG"
step "just preflight"
just preflight > "$SP/img-preflight.log" 2>&1; echo "exit preflight $?" >> "$LOG"
tail -4 "$SP/img-preflight.log" >> "$LOG"
step "pw-core, every test; pw-cli's guide; pw-syntax's corpus formatting"
cargo test --locked --no-fail-fast -p pw-core > "$SP/img-pw-core.log" 2>&1; echo "exit pw-core $?" >> "$LOG"
grep -E "^test result" "$SP/img-pw-core.log" | awk '{p+=$4; f+=$6; i+=$8} END {print "pw-core: " p " passed, " f " failed, " i " ignored"}' >> "$LOG"
grep -E "FAILED|panicked at" "$SP/img-pw-core.log" | head -20 >> "$LOG"
cargo test --locked -p pw-cli --test guide > "$SP/img-guide2.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "^test result" "$SP/img-guide2.log" >> "$LOG"
cargo test --locked -p pw-syntax --test corpus_fmt > "$SP/img-fmt2.log" 2>&1; echo "exit corpus_fmt $?" >> "$LOG"
grep -E "^test result" "$SP/img-fmt2.log" >> "$LOG"
step "just evidence-gates"
just evidence-gates > "$SP/img-evidence-gates.log" 2>&1; echo "exit evidence-gates $?" >> "$LOG"
grep -E "^Ran [0-9]+ tests|^OK|^FAILED|^# (tests|pass|fail)|^ERROR:|^FAIL:" "$SP/img-evidence-gates.log" >> "$LOG"
# images and guide changed themselves, so they run whatever reached.py says.
for s in $( (sed -n 's/^\([a-z_0-9]*\)_mutations\.py: .*/\1/p' "$SP/img-reached.txt"; echo images; echo guide) | sort -u); do
  step "mutation controls: $s"
  CARGO_INCREMENTAL=0 python3 "scripts/${s}_mutations.py" > "$SP/img-mut-$s.log" 2>&1
  echo "exit $s $?" >> "$LOG"
  grep -E "KILLED|SURVIVED|REFUSED|DID NOT BUILD|mutants killed|killed;|FAIL" "$SP/img-mut-$s.log" | tail -90 >> "$LOG"
done
step "restored"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
git status --porcelain -- . ':(exclude)docs' >> "$LOG"
echo IMG_VERIFY_FINISHED >> "$LOG"
