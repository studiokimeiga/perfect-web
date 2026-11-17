# ADR-0338's D3 (track `d3-postgres`): any program's declared tables on
# PostgreSQL, behind the memory adapter's interface
# (`runtime/pw-table-postgres`). The root justfile imports this file; an
# evidence recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and
# is planned by the verification run like any other, beside a database
# (`scripts/ci_plan.py`'s NEEDS_DATABASE).

# D3: the schema made from tables.json once under its lock, its changes made
# or refused, drift refused at start; every primitive one prepared statement
# at two sizes; a command one serializable transaction with the platform's
# records; a render's snapshot in a bounded few seats; the same answers as
# the memory adapter (a seeded workload, examples/tables' compiled programs);
# a commit outliving its host; then the mutation controls of D3's own, part 1
# of 2 (scripts/d3_postgres_mutations.py --part 1/2; part 2, the uploads',
# is e14-d3-postgres-2's: ADR-0393, amended 2026-11-17). It needs a
# throwaway PostgreSQL database, PW_TABLES_DATABASE_URL or
# PW_STORE_DATABASE_URL (each test makes a schema of its own and drops it);
# it skips without one, and writes no evidence.
e14-d3-postgres:
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     if [ -z "$url" ]; then \
       echo "e14-d3-postgres: skipped, PW_STORE_DATABASE_URL is not set (a throwaway PostgreSQL database, e.g. postgresql://localhost/pw_store_test)"; \
       exit 0; \
     fi; \
     export PW_TABLES_DATABASE_URL="$url"; \
     mkdir -p docs/evidence/E14; \
     report="$(mktemp docs/evidence/E14/.d3-postgres.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "D3 - any program's declared tables on PostgreSQL, answering as the memory adapter does"; echo; \
       echo "produced by: just e14-d3-postgres"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       for binary in primitives commands migrations parity durability compiled_owned counts compiled_counts compiled_windows compiled_ranges compiled_relations compiled_assets compiled_cases asset_rows asset_enrollment platform; do \
         echo "== tests/$binary.rs"; \
         cargo test --locked -p pw-table-postgres --test "$binary" 2>&1 | grep -E '^(test |test result)|panicked at'; \
         echo; \
       done; \
       echo "== mutation controls, part 1 of 2 (scripts/d3_postgres_mutations.py --part 1/2; part 2 is e14-d3-postgres-2's)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/d3_postgres_mutations.py --part 1/2; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/d3-postgres.txt
    @grep -E '^test result|mutants killed|^baseline:|panicked at' docs/evidence/E14/d3-postgres.txt

# D3's mutation controls, part 2 of 2 (ADR-0393, amended 2026-11-17): the
# uploads' mutants (ADR-0440's L5), in a job of their own, with the baseline
# every part runs. The same database as e14-d3-postgres's; it skips without
# one, and writes no evidence.
e14-d3-postgres-2:
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     if [ -z "$url" ]; then \
       echo "e14-d3-postgres-2: skipped, PW_STORE_DATABASE_URL is not set (a throwaway PostgreSQL database, e.g. postgresql://localhost/pw_store_test)"; \
       exit 0; \
     fi; \
     export PW_TABLES_DATABASE_URL="$url"; \
     mkdir -p docs/evidence/E14; \
     report="$(mktemp docs/evidence/E14/.d3-postgres-2.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "D3 - any program's declared tables on PostgreSQL: its mutation controls, part 2 of 2, the uploads'"; echo; \
       echo "produced by: just e14-d3-postgres-2"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== mutation controls, part 2 of 2 (scripts/d3_postgres_mutations.py --part 2/2; part 1 is e14-d3-postgres's)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/d3_postgres_mutations.py --part 2/2; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/d3-postgres-2.txt
    @grep -E 'mutants killed|^baseline:|panicked at' docs/evidence/E14/d3-postgres-2.txt
