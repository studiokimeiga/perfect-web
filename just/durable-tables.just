# Track `durable-tables` (ADR-0338's D3 joined to D2's routing): a program's
# declared tables served durably from PostgreSQL by the development server,
# through the same gate as in memory (`server/src/tables_pg.rs`, over
# `runtime/pw-table-postgres`), chosen where the deployment names a database
# (`PW_TABLES_DATABASE_URL`). The root justfile imports this file; an evidence
# recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`, and is
# planned by the verification run like any other, beside a database
# (`scripts/ci_plan.py`'s NEEDS_DATABASE).

# A command's rows, its runs, its interaction's record and its outbox in one
# PostgreSQL transaction; one state a render; the reader's session the
# database's, each command fenced on it; the adapter's runs, sessions and
# fence (tests/platform.rs); the server on PostgreSQL (tests::tables_pg::, the
# conformance suite over it among them) and still in memory; the server as a
# process, killed and stopped (tests/durable_tables.rs); the guestbook in
# three engines on PostgreSQL (its own spec, as written, and a restart with a
# page open) and in memory (the main suite's hosts keep their tables there,
# whatever the caller names: host-plan.mjs); then every mutation control,
# whole (scripts/durable_tables_mutations.py). It needs a throwaway PostgreSQL
# database, PW_TABLES_DATABASE_URL (or PW_STORE_DATABASE_URL, which CI's
# database shards name): refused without one. A suite or a test that fails
# fails the recipe, and the evidence it wrote says so.
e14-durable-tables:
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     if [ -z "$url" ]; then \
       echo "e14-durable-tables: refused, it needs PW_TABLES_DATABASE_URL (or PW_STORE_DATABASE_URL), a throwaway PostgreSQL database"; \
       exit 1; \
     fi
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/guestbook.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     export PW_TABLES_DATABASE_URL="$url"; \
     { echo "durable-tables - a program's declared tables served durably from PostgreSQL, through the gate: one transaction a command, one state a render, the reader's session the database's"; echo; \
       echo "produced by: just e14-durable-tables"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== the adapter's runs, sessions and fence (runtime/pw-table-postgres/tests/platform.rs)"; echo; \
       cargo test --locked -p pw-table-postgres --test platform 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the server on PostgreSQL (tests::tables_pg::), the conformance suite over it"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::tables_pg:: --nocapture 2>&1 | grep -E '^(test |test result)|panicked at|^conformance'; \
       echo; echo "== and in memory (tests::tables::, tests::guestbook::, tests::conformance::)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::tables:: tests::guestbook:: tests::conformance:: --nocapture 2>&1 | grep -E '^(test |test result)|panicked at|^conformance'; \
       echo; echo "== the server as a process, killed and stopped (spikes/own-renderer/server/tests/durable_tables.rs)"; echo; \
       cargo test --locked -p pw-dev-server --test durable_tables 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== the guestbook in three engines on PostgreSQL (playwright.tables.config.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test --config playwright.tables.config.mjs e2e/guestbook.spec.mjs e2e-tables/guestbook-durable.spec.mjs --reporter=line; \
       echo; echo "== and in memory, the main suite's hosts (playwright.config.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/guestbook.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/durable_tables_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/durable_tables_mutations.py; \
     } > docs/evidence/E14/durable-tables.txt
    @grep -E '^test result|passed|failed|mutants killed|^baseline:|^conformance|panicked at' docs/evidence/E14/durable-tables.txt
