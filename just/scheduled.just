# The scheduled-commands track's recipes (the integrator's): a command runs
# when the clock passes its time. The root justfile imports this file; an
# evidence recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`,
# and is planned by the verification run like any other.

# `invoked_by Clock`, and `schedule cmd(args) at when`, `schedule cmd(args)
# after duration` and `unschedule cmd(args)`, statements of a command's body
# (PW65xx); the host's runs, kept with the scheduling command's writes and
# run on its user's behalf; and the store's sweep, an order no one pays
# taken back thirty minutes on. The compiler's tests (tests/scheduled.rs),
# the server's (tests/store_scheduled.rs, schedules.rs) on each layer, the
# database's own (tests/store_pg.rs, tests/feed_pg.rs), and the mutation
# controls (scripts/scheduled_mutations.py), which need a database.
e14-scheduled:
    @url="${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}"; \
     if [ -z "$url" ]; then \
       echo "e14-scheduled: skipped, PW_STORE_DATABASE_URL is not set (a throwaway PostgreSQL database, e.g. postgresql://localhost/pw_store_test)"; \
       exit 0; \
     fi; \
     export PW_STORE_DATABASE_URL="$url" PW_FEED_DATABASE_URL="$url"; \
     mkdir -p docs/evidence/E14; \
     { echo "The scheduled-commands track - a command runs when the clock passes its time"; echo; \
       echo "produced by: just e14-scheduled"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== the statements and their refusals (compiler/pw-core/tests/scheduled.rs)"; echo; \
       cargo test --locked -p pw-core --test scheduled 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the host's runs and the store's sweep, the store in memory (tests/store_scheduled.rs, schedules.rs)"; echo; \
       PW_STORE_TEST_LAYER=memory cargo test --locked -p pw-dev-server -- tests::store_scheduled:: schedules:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the same, the store on PostgreSQL (PW_STORE_TEST_LAYER=postgres)"; echo; \
       PW_STORE_TEST_LAYER=postgres cargo test --locked -p pw-dev-server -- tests::store_scheduled:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== a run in the database: its transaction, a restart, two hosts, the feed (tests/store_pg.rs, tests/feed_pg.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- tests::store_pg::on_postgres_a_run tests::store_pg::on_postgres_two_hosts tests::feed_pg::on_postgres_a_feeds_run 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== mutation controls (scripts/scheduled_mutations.py)"; echo; \
       python3 scripts/scheduled_mutations.py; \
     } > docs/evidence/E14/scheduled.txt; \
     grep -E "^test result|mutants killed|panicked at" docs/evidence/E14/scheduled.txt
