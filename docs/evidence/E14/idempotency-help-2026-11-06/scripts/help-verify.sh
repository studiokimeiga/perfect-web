#!/usr/bin/env bash
# The InteractionId help change at its exact commit: formatting, the
# compiler's tests whole, the guide's test, the help followed as a reader
# would with the built `pw`, `just preflight`, `just evidence-gates`, and
# every mutation script the change reaches (`reached.py`, against master),
# each run whole.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/help
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/idempotency-help
LOG="$SP/help-verify.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "commit $(git rev-parse HEAD); uncommitted code paths: $(git status --porcelain -- . ':(exclude)docs' | wc -l)"
echo "rust: $(rustc --version); node: $(node --version)" >> "$LOG"
step "reached mutation scripts (reached.py against master, 3 lines)"
python3 "$SP/reached.py" 5d4d4d55a36f52ea556aa8f2af3e70b1150ffcf6 3 > "$SP/help-reached.txt" 2>&1
cat "$SP/help-reached.txt" >> "$LOG"
step "cargo fmt --check"
cargo fmt --all --check >> "$LOG" 2>&1; echo "exit fmt $?" >> "$LOG"
step "build pw"
cargo build --locked -p pw-cli > "$SP/help-build.log" 2>&1; echo "exit build $?" >> "$LOG"
step "the help, followed as a reader would"
python3 "$SP/help_demo.py" "$CARGO_TARGET_DIR/debug/pw" "$SP/help-demo" >> "$LOG" 2>&1; echo "exit help_demo $?" >> "$LOG"
step "pw-core, every test; pw-cli's guide"
cargo test --locked --no-fail-fast -p pw-core > "$SP/help-pw-core.log" 2>&1; echo "exit pw-core $?" >> "$LOG"
grep -E "^test result" "$SP/help-pw-core.log" | awk '{p+=$4; f+=$6; i+=$8} END {print "pw-core: " p " passed, " f " failed, " i " ignored"}' >> "$LOG"
grep -E "FAILED|panicked at" "$SP/help-pw-core.log" | head -20 >> "$LOG"
cargo test --locked -p pw-cli --test guide > "$SP/help-guide.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "^test result|FAILED|panicked at" "$SP/help-guide.log" >> "$LOG"
step "just preflight"
just preflight > "$SP/help-preflight.log" 2>&1; echo "exit preflight $?" >> "$LOG"
tail -4 "$SP/help-preflight.log" >> "$LOG"
step "just evidence-gates"
just evidence-gates > "$SP/help-evidence-gates.log" 2>&1; echo "exit evidence-gates $?" >> "$LOG"
grep -E "^Ran [0-9]+ tests|^OK|^FAILED|^# (tests|pass|fail)" "$SP/help-evidence-gates.log" >> "$LOG"
for s in $(sed -n 's/^\([a-z_0-9]*\)_mutations\.py: .*/\1/p' "$SP/help-reached.txt"); do
  step "mutation controls: $s"
  CARGO_INCREMENTAL=0 python3 "scripts/${s}_mutations.py" > "$SP/help-mut-$s.log" 2>&1
  echo "exit $s $?" >> "$LOG"
  grep -E "KILLED|SURVIVED|REFUSED|DID NOT BUILD|mutants killed" "$SP/help-mut-$s.log" | tail -60 >> "$LOG"
done
step "restored"
python3 scripts/mutation_anchors.py >> "$LOG" 2>&1; echo "exit anchors $?" >> "$LOG"
git status --porcelain -- . ':(exclude)docs' >> "$LOG"
echo HELP_VERIFY_FINISHED >> "$LOG"
