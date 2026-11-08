#!/usr/bin/env bash
# The stale-key change at its exact commit: `just preflight`, `just
# evidence-gates`, the compiler's tests whole, and every mutation script the
# change reaches (`reached.py`, against master), each run whole.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/site
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/stale-key
LOG="$SP/stale-verify.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "commit $(git rev-parse HEAD); uncommitted code paths: $(git status --porcelain -- . ':(exclude)docs' | wc -l)"
echo "rust: $(rustc --version); node: $(node --version)" >> "$LOG"
step "reached mutation scripts (reached.py against master, 3 lines)"
python3 "$SP/reached.py" 5d4d4d55a36f52ea556aa8f2af3e70b1150ffcf6 3 > "$SP/stale-reached.txt" 2>&1
cat "$SP/stale-reached.txt" >> "$LOG"
step "just preflight"
just preflight > "$SP/stale-preflight.log" 2>&1; echo "exit preflight $?" >> "$LOG"
tail -4 "$SP/stale-preflight.log" >> "$LOG"
step "just evidence-gates"
just evidence-gates > "$SP/stale-evidence-gates.log" 2>&1; echo "exit evidence-gates $?" >> "$LOG"
grep -E "^Ran [0-9]+ tests|^OK|^FAILED|^# (tests|pass|fail)" "$SP/stale-evidence-gates.log" >> "$LOG"
step "pw-core, pw-cli's guide, pw-syntax's corpus formatting"
cargo test --locked --no-fail-fast -p pw-core > "$SP/stale-pw-core2.log" 2>&1; echo "exit pw-core $?" >> "$LOG"
grep -E "^test result" "$SP/stale-pw-core2.log" | awk '{p+=$4; f+=$6; i+=$8} END {print "pw-core: " p " passed, " f " failed, " i " ignored"}' >> "$LOG"
cargo test --locked -p pw-cli --test guide > "$SP/stale-guide2.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "^test result" "$SP/stale-guide2.log" >> "$LOG"
cargo test --locked -p pw-syntax --test corpus_fmt > "$SP/stale-fmt2.log" 2>&1; echo "exit corpus_fmt $?" >> "$LOG"
grep -E "^test result" "$SP/stale-fmt2.log" >> "$LOG"
# one_checker's script changed itself (a mutant retargeted), so it runs too.
for s in $( (sed -n 's/^\([a-z_0-9]*\)_mutations\.py: .*/\1/p' "$SP/stale-reached.txt"; echo one_checker) | sort -u); do
  step "mutation controls: $s"
  CARGO_INCREMENTAL=0 python3 "scripts/${s}_mutations.py" > "$SP/stale-mut-$s.log" 2>&1
  echo "exit $s $?" >> "$LOG"
  grep -E "KILLED|SURVIVED|REFUSED|DID NOT BUILD|mutants killed" "$SP/stale-mut-$s.log" | tail -60 >> "$LOG"
done
step "restored"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
git status --porcelain -- . ':(exclude)docs' >> "$LOG"
echo STALE_VERIFY_FINISHED >> "$LOG"
