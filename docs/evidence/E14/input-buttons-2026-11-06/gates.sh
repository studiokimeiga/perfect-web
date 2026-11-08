#!/usr/bin/env bash
# The input-button ADR's gates at the commit under test, run from the
# worktree's root with a Cargo target of its own. Each writes OUT/<file>
# with a header naming the commit, the command's whole output and its exit
# status. The two long logs are kept whole and gzipped beside a summary.
#
#   bash docs/evidence/E14/input-buttons-2026-11-06/gates.sh TARGET OUT
set -uo pipefail
target=$1 out=$2
. "$HOME/pw-setup/env.sh"
export CARGO_TARGET_DIR="$target"
mkdir -p "$out"

header() {
    echo "$1"
    echo
    echo "produced by: $2 (gates.sh, in $PWD)"
    echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"
    echo "rust: $(rustc --version)"
    echo "started: $(date -u +%FT%TZ)"
    echo
}

run() {
    local file=$1 title=$2
    shift 2
    {
        header "$title" "$*"
        "$@"
        echo "exit: $?"
        echo "finished: $(date -u +%FT%TZ)"
    } > "$out/$file" 2>&1
}

run pw-core-tests.log "cargo test -p pw-core, whole" cargo test --locked --no-fail-fast -p pw-core
run guide.txt "the guide's test" cargo test --locked -p pw-cli --test guide
run mutation-anchors.txt "every mutation script's anchors, each matching once" python3 scripts/mutation_anchors.py
run preflight.txt "just preflight" just preflight
run evidence-gates.log "just evidence-gates" just evidence-gates

# The summaries: each test binary's result, and any failure, with the header
# and exit status; the whole logs beside them, gzipped.
for name in pw-core-tests evidence-gates; do
    {
        head -n 7 "$out/$name.log"
        echo "(the whole log: $name.log.gz)"
        echo
        grep -E '^     Running |^test result|^Ran [0-9]+ tests|^OK|^FAILED|^FAIL:|^ERROR:|panicked at|^exit:|^finished:' "$out/$name.log"
    } > "$out/$name.txt"
    gzip -n -f "$out/$name.log"
done
