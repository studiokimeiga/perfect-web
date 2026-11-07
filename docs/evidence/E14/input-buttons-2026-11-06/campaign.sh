#!/usr/bin/env bash
# The input-button ADR's mutation campaigns: each script named, run whole, one
# after another, in a worktree of its own (a detached checkout of the commit
# under test) with a Cargo target of its own, so that two campaigns never
# mutate one source tree. Each writes OUT/<script>.txt: a header naming the
# commit, the script's whole output and its exit status, then the anchor
# check, which also says the source was restored.
#
#   bash campaign.sh WORKTREE TARGET OUT script ...
#   e.g. bash campaign.sh /home/haki/code/pw-wt/pw5014-mut-b \
#            /home/haki/pw-targets/pw5014-mut-b OUT labels ids
set -uo pipefail
wt=$1 target=$2 out=$3
shift 3
cd "$wt" || exit 2
. "$HOME/pw-setup/env.sh"
export CARGO_TARGET_DIR="$target"
mkdir -p "$out"
for name in "$@"; do
    script="scripts/${name}_mutations.py"
    file="$out/${name//_/-}-mutations.txt"
    {
        echo "$script, run whole"
        echo
        echo "produced by: bash campaign.sh $wt $target OUT $name"
        echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"
        echo "rust: $(rustc --version)"
        echo "CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-} PW_MUTANT_INCREMENTAL=${PW_MUTANT_INCREMENTAL:-}"
        echo "started: $(date -u +%FT%TZ)"
        echo
        python3 "$script"
        echo "exit: $?"
        echo "finished: $(date -u +%FT%TZ)"
        echo
        echo "== python3 scripts/mutation_anchors.py, after"
        python3 scripts/mutation_anchors.py
        echo "exit: $?"
        echo "source: $(git diff --quiet HEAD -- . ':(exclude)docs/evidence' && echo 'as committed' || echo 'CHANGED')"
    } > "$file" 2>&1
done
