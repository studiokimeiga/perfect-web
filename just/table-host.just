# ADR-0338 D2's read path (PRs #29 to #40, integrated, and the integrator's
# rulings of 2026-10-25 on them): a compiled component reads its tables
# through the table gate, typed, exact, and held to tables.json; the memory
# adapter answers it. The tests, then every mutation control, whole.
e14-table-host:
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.table-host.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "D2 - a compiled component reads its tables through the table gate"; echo; \
       echo "produced by: just e14-table-host"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       cargo test --locked -p pw-table-memory; \
       cargo test --locked -p pw-host --features engine --lib; \
       cargo test --locked -p pw-core --test table_host_reader_fixture; \
       echo; echo "== mutation controls (scripts/table_host_mutations.py)"; \
       python3 scripts/table_host_mutations.py; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/table-host.txt
    @grep -E 'test result|mutants killed|^baseline:' docs/evidence/E14/table-host.txt
