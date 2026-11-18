# ADR-0245, amended 2026-11-18: a campaign's bundle (a recipe's `mktemp -d`
# directory under docs/evidence) is kept while a record names it, and a
# fetch prunes the bundles its evidence superseded (`evidence_bundles.py`,
# `evidence_fetch.py`). Their tests, then the mutation controls
# (scripts/evidence_bundles_mutations.py).
e14-evidence-bundles:
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.evidence-bundles.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; mv "$report" docs/evidence/E14/evidence-bundles.txt; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "evidence-bundles - a campaign's bundle is kept while a record names it"; echo; \
       echo "produced by: just e14-evidence-bundles"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo; python3 -m unittest -v scripts/tests/test_evidence_bundles.py; \
       echo; echo "== mutation controls (scripts/evidence_bundles_mutations.py)"; \
       python3 scripts/evidence_bundles_mutations.py; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/evidence-bundles.txt
    @grep -E 'mutants killed|^baseline:|^Ran |^OK' docs/evidence/E14/evidence-bundles.txt
