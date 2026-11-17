# Track `row-ownership` (ADR-0384's H1 and H2): a table states whose its
# rows are, who else reads them and what its owner alone reads; every read
# and write of an owned table is by its reader's handle, held by the host to
# the call's caller; a predicate the program gives a body is asked by the
# host before the command or query it guards; a query that requires one is
# read per reader and by a page's binding alone, which the development
# server's own tests hold (`required_queries`, outside the mutation
# script's binaries: a failure there fails the recipe). The tests, then the
# mutation controls' first part; `e14-row-ownership-2` and
# `e14-row-ownership-3` run the other two (`PARTS`, ADR-0359, amended
# 2026-11-17: whole, the script was every full verify's longest recipe, about
# three hours), each after a baseline of its own.
e14-row-ownership:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/owned.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.row-ownership.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; mv "$report" docs/evidence/E14/row-ownership.txt; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "row-ownership - owned tables read and written by their reader; predicates in Pleris"; echo; \
       echo "produced by: just e14-row-ownership"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       cargo test --locked -p pw-core --test table_owners --test table_counts --test table_windows --test table_ranges --test table_relations --test table_assets; \
       cargo test --locked -p pw-core --test predicate_bodies; \
       cargo test --locked -p pw-host --features engine --lib table_owners; \
       cargo test --locked -p pw-table-memory --test compiled_owners --test compiled_counts --test compiled_windows --test compiled_ranges --test compiled_relations --test compiled_assets --test compiled_cases; \
       cargo test --locked -p pw-table-memory --test compiled_predicates; \
       cargo test --locked -p pw-dev-server required_queries || exit 1; \
       echo; echo "== owned rows in Chromium, Firefox and WebKit"; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/owned-rows.spec.mjs --workers=3; \
       echo; echo "== mutation controls (scripts/row_ownership_mutations.py), part 1 of 3"; \
       python3 scripts/row_ownership_mutations.py --part 1/3; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/row-ownership.txt
    @grep -E 'test result|mutants killed|^baseline:|^part ' docs/evidence/E14/row-ownership.txt

# Part 2 of 3 of the row-ownership mutation controls (`e14-row-ownership`
# runs the tests and part 1), on the same builds, after a baseline of its own.
e14-row-ownership-2:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/owned.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.row-ownership-2.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; mv "$report" docs/evidence/E14/row-ownership-2.txt; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "row-ownership - owned tables read and written by their reader; predicates in Pleris: mutation controls, part 2 of 3"; echo; \
       echo "produced by: just e14-row-ownership-2"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== mutation controls (scripts/row_ownership_mutations.py), part 2 of 3"; \
       python3 scripts/row_ownership_mutations.py --part 2/3; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/row-ownership-2.txt
    @grep -E 'mutants killed|^baseline:|^part ' docs/evidence/E14/row-ownership-2.txt

# Part 3 of 3 of the row-ownership mutation controls (`e14-row-ownership`
# runs the tests and part 1), on the same builds, after a baseline of its own.
e14-row-ownership-3:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/owned.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.row-ownership-3.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; mv "$report" docs/evidence/E14/row-ownership-3.txt; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "row-ownership - owned tables read and written by their reader; predicates in Pleris: mutation controls, part 3 of 3"; echo; \
       echo "produced by: just e14-row-ownership-3"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       echo "== mutation controls (scripts/row_ownership_mutations.py), part 3 of 3"; \
       python3 scripts/row_ownership_mutations.py --part 3/3; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/row-ownership-3.txt
    @grep -E 'mutants killed|^baseline:|^part ' docs/evidence/E14/row-ownership-3.txt
