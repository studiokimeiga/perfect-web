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
# a commit outliving its host; then the mutation controls' part 1 of 3, the
# first half of D3's own (scripts/d3_postgres_mutations.py --part 1/3; part 2,
# their second half, is e14-d3-postgres-2's, part 3, the uploads',
# e14-d3-postgres-3's: ADR-0393 and ADR-0359, amended 2026-11-17). It needs a
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
       echo "== the crate's own (src, --lib)"; \
       cargo test --locked -p pw-table-postgres --lib 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; \
       for path in runtime/pw-table-postgres/tests/*.rs; do \
         binary="$(basename "$path" .rs)"; \
         echo "== tests/$binary.rs"; \
         cargo test --locked -p pw-table-postgres --test "$binary" 2>&1 | grep -E '^(test |test result)|panicked at'; \
         echo; \
       done; \
       echo "== mutation controls, part 1 of 3 (scripts/d3_postgres_mutations.py --part 1/3; parts 2 and 3 are e14-d3-postgres-2's and -3's)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/d3_postgres_mutations.py --part 1/3; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/d3-postgres.txt; \
     grep -E '^test result|mutants killed|^baseline:|panicked at' docs/evidence/E14/d3-postgres.txt

# D3's mutation controls, part 2 of 3 (ADR-0359, amended 2026-11-17): the
# second half of D3's own mutants, a recipe of their own, with the baseline
# every part runs, dealt as any recipe is beside a database of its shard's;
# it skips without one, and writes no evidence.
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
     { echo "D3 - any program's declared tables on PostgreSQL: its mutation controls, part 2 of 3, the second half of D3's own"; echo; \
       echo "produced by: just e14-d3-postgres-2"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== mutation controls, part 2 of 3 (scripts/d3_postgres_mutations.py --part 2/3; parts 1 and 3 are e14-d3-postgres's and -3's)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/d3_postgres_mutations.py --part 2/3; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/d3-postgres-2.txt; \
     grep -E 'mutants killed|^baseline:|panicked at' docs/evidence/E14/d3-postgres-2.txt

# D3's mutation controls, part 3 of 3 (ADR-0393 and ADR-0359, amended 2026-11-17): the
# uploads' mutants (ADR-0440's L5), a recipe of their own, with the baseline
# every part runs, dealt as any recipe is beside a database of its shard's;
# it skips without one, and writes no evidence.
e14-d3-postgres-3:
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     if [ -z "$url" ]; then \
       echo "e14-d3-postgres-3: skipped, PW_STORE_DATABASE_URL is not set (a throwaway PostgreSQL database, e.g. postgresql://localhost/pw_store_test)"; \
       exit 0; \
     fi; \
     export PW_TABLES_DATABASE_URL="$url"; \
     mkdir -p docs/evidence/E14; \
     report="$(mktemp docs/evidence/E14/.d3-postgres-3.XXXXXX)"; \
     trap 'status=$?; if [ "$status" -ne 0 ]; then cat "$report" >&2; fi; rm -f "$report"; exit "$status"' EXIT; \
     { echo "D3 - any program's declared tables on PostgreSQL: its mutation controls, part 3 of 3, the uploads'"; echo; \
       echo "produced by: just e14-d3-postgres-3"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== mutation controls, part 3 of 3 (scripts/d3_postgres_mutations.py --part 3/3; parts 1 and 2 are e14-d3-postgres's and -2's)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/d3_postgres_mutations.py --part 3/3; \
     } > "$report" 2>&1; \
     mv "$report" docs/evidence/E14/d3-postgres-3.txt; \
     grep -E 'mutants killed|^baseline:|panicked at' docs/evidence/E14/d3-postgres-3.txt
