#!/usr/bin/env bash
# The requires change before its commit: pw-core whole, the guide, the corpus
# formatting, and which tests fail, by name, with each panic's first lines.
set -uo pipefail
export PATH="$HOME/.nvm/versions/node/v22.21.1/bin:$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export CARGO_TARGET_DIR=/home/haki/pw-targets/requires
SP=/mnt/c/Users/haki/AppData/Local/Temp/claude/a3a99843d7659d2c/c2fdf789-8c40-4584-932e-eb0f5a7419a5/scratchpad
cd /home/haki/code/pw-wt/requires
LOG="$SP/requires-tests.log"
: > "$LOG"
step() { echo "== $(date -Is) $*" >> "$LOG"; }
step "pw-core, every test"
cargo test --locked --no-fail-fast -p pw-core > "$SP/requires-pw-core.log" 2>&1; echo "exit pw-core $?" >> "$LOG"
grep -E "^test result" "$SP/requires-pw-core.log" | awk '{p+=$4; f+=$6} END {print "pw-core: " p " passed, " f " failed"}' >> "$LOG"
grep -E "^test .* FAILED$" "$SP/requires-pw-core.log" | sort -u >> "$LOG"
grep -E "Running (tests|unittests)" "$SP/requires-pw-core.log" > /dev/null
step "pw-cli: the guide; pw-syntax: corpus formatting"
cargo test --locked -p pw-cli --test guide > "$SP/requires-guide.log" 2>&1; echo "exit guide $?" >> "$LOG"
grep -E "^test result|FAILED" "$SP/requires-guide.log" >> "$LOG"
cargo test --locked -p pw-syntax --test corpus_fmt > "$SP/requires-fmt.log" 2>&1; echo "exit corpus_fmt $?" >> "$LOG"
grep -E "^test result|FAILED" "$SP/requires-fmt.log" >> "$LOG"
echo REQUIRES_TESTS_FINISHED >> "$LOG"
