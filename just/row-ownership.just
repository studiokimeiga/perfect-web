# Track `row-ownership` (ADR-0384's H1 and H2): a table states whose its
# rows are, who else reads them and what its owner alone reads; every read
# and write of an owned table is by its reader's handle, held by the host to
# the call's caller; a predicate the program gives a body is asked by the
# host before the command or query it guards. The tests, then every
# mutation control, whole.
e14-row-ownership:
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.row-ownership.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "row-ownership - owned tables read and written by their reader; predicates in Pleris"; echo; \
       echo "produced by: just e14-row-ownership"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       cargo test --locked -p pw-core --test table_owners; \
       cargo test --locked -p pw-core --test predicate_bodies; \
       cargo test --locked -p pw-host --features engine --lib table_owners; \
       cargo test --locked -p pw-table-memory --test compiled_owners; \
       cargo test --locked -p pw-table-memory --test compiled_predicates; \
       echo; echo "== mutation controls (scripts/row_ownership_mutations.py)"; \
       python3 scripts/row_ownership_mutations.py; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/row-ownership.txt
    @grep -E 'test result|mutants killed|^baseline:' docs/evidence/E14/row-ownership.txt
