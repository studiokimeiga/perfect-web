# ADR-0338 D1: the compiler half of declared data, without an adapter.
e14-tables:
    @mkdir -p docs/evidence/E14; \
     { echo "D1 - a table declares an app's data"; echo; \
       echo "produced by: just e14-tables"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       cargo test --locked -p pw-core --test tables --test table_counts --test table_windows -- --nocapture; \
       cargo test --locked -p pw-cli --test tables -- --nocapture; \
       echo; echo "== mutation controls (scripts/tables_mutations.py)"; \
       python3 scripts/tables_mutations.py; \
     } > docs/evidence/E14/tables.txt 2>&1; \
     grep -E 'test result|mutants killed|^baseline:' docs/evidence/E14/tables.txt
