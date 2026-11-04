# Track `data-path` (ADR-0338's D2: an app with no Rust data layer): the
# development server serves a program's declared tables through the table
# gate alone, a command's rows published as its transaction's last step
# before its COMMIT, after its every record and its outbox, and the tables
# closed where the COMMIT fails after; a guarded command answered before its
# tables are linked; one render reading one state of the tables; the guide's
# guestbook on the tables it declares, bounded, its refusals said, the
# tutorial byte for byte the app; the reader's account, never a guest's or a
# stand-in's, and a guest's identity never its session; nothing the host
# keeps, shows or logs made of the session's cookie (in memory, and on
# PostgreSQL where PW_FEED_DATABASE_URL names a throwaway database: skipped,
# and said so, without one); no row holding a
# handle, at the compiler (PW6901) and at the host; the conformance suite
# over the `DataLayer` seam, for the layer under test, generated for every
# declared table of the guestbook and the lists app; the guestbook in three
# engines; then every mutation control, whole. Everything the track's ADR
# claims is recorded here (the integrator's ruling S4 of 2026-11-02). Each
# step's failure stops the recipe, and the evidence keeps what it said, a
# failed browser suite's count among it (track evidence-integrity-2).
e14-data-path:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/guestbook.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ export PW_STORE_DATABASE_URL="${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}"; \
       echo "D2 - an app with no Rust data layer: declared tables served through the gate, the guestbook on them, the conformance suite"; echo; \
       echo "produced by: just e14-data-path"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; \
       if [ -n "${PW_FEED_DATABASE_URL:-}" ]; then \
         echo "postgres: $(psql "$PW_FEED_DATABASE_URL" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; \
       else echo "postgres: PW_FEED_DATABASE_URL is not set, so the PostgreSQL tests pass doing nothing"; fi; echo; \
       echo "== the server: routing, the guestbook, the conformance suite, the reader's account, a guest's identity, the session's cookie kept nowhere"; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::tables:: tests::guestbook:: tests::conformance:: notifications::tests:: tables::tests:: tests::sign_in::nothing_the_feed_keeps_holds_a_session_cookie tests::sign_in::on_postgres::nothing_kept_on_postgres_holds_a_session_cookie tests::sign_in::each_feed_operation_answers_its_requests_session_alone tests::sign_in::on_postgres::each_feed_operation_answers_its_requests_session_alone_on_postgres tests::store_accounts::nothing_a_session_keyed_store_keeps_holds_a_session_cookie tests::store_pg::on_postgres_nothing_the_store_keeps_holds_a_session_cookie uploads::tests::a_claim_no_transaction_commits_is_given_back --test-threads=8 --nocapture; \
       echo; echo "== the engine: a guarded command through the gate; the adapter's snapshot"; \
       cargo test --locked -p pw-table-memory --test compiled_guarded --test compiled_snapshot --test compiled_ranges; \
       echo; echo "== the platform's transaction: a command's last step after its records, before its COMMIT"; \
       cargo test --locked -p pw-materialize --lib -- a_commands_last_step; \
       echo; echo "== the compiler: no row holds a handle (PW6901); the reader's account, labelled the reader's; the block's owner"; \
       cargo test --locked -p pw-core --test handles --test platform_contracts; \
       cargo test --locked -p pw-core --lib -- codes::tests; \
       echo; echo "== the tutorial is the app (compiler/pw-cli/tests/guide.rs); the reference of codes"; \
       cargo test --locked -p pw-cli --test guide; \
       python3 scripts/guide_diagnostics.py --check; \
       echo; echo "== the guestbook, examples/guestbook/*.pw, in three engines (e2e/guestbook.spec.mjs)"; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/guestbook.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/data_path_mutations.py)"; \
       python3 scripts/data_path_mutations.py; \
     } > docs/evidence/E14/data-path.txt 2>&1
    @grep -E 'test result|mutants killed|^baseline:|^conformance:|at its bound|passed|flaky|failed' docs/evidence/E14/data-path.txt
