# ADR-0386, D2's writes and other reads (track `d2-writes`): a compiled
# command writes its tables through the table gate, typed, staged in its own
# command and published whole at its commit or not at all; every read
# primitive is gated, typed and held to what it was asked; the memory
# adapter answers them. The tests, the compiler's tables.json executed by the
# adapter, then every mutation control, whole.
e14-d2-writes:
    @mkdir -p docs/evidence/E14
    @report="$(mktemp docs/evidence/E14/.d2-writes.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "D2 - a command writes its tables through the table gate, staged; every read gated"; echo; \
       echo "produced by: just e14-d2-writes"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       cargo test --locked -p pw-table-memory; \
       cargo test --locked -p pw-host --features engine --lib; \
       cargo test --locked -p pw-cli --test tables; \
       echo; echo "== mutation controls (scripts/d2_writes_mutations.py)"; \
       python3 scripts/d2_writes_mutations.py; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/d2-writes.txt
    @grep -E 'test result|mutants killed|^baseline:' docs/evidence/E14/d2-writes.txt
