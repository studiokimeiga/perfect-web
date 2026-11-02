# Track `session-overlap`'s recipes (ADR-0253, docs/PARALLEL.md). The track
# adds its recipes here and nowhere else; the root justfile imports this
# file, and its settings and variables (`set shell`, `PATH`) apply here as
# there. An evidence recipe is `e14-<name>:`, writes
# `docs/evidence/E14/<name>.txt`, and is planned by the verification run like
# any other (scripts/ci_plan.py reads this file too).

# Track `session-overlap` (ADR-0389, amended 2026-11-02, and its review): two
# requests of one session whose admissions overlap are each answered, every
# request is admitted, the runtime's own files too, and on PostgreSQL an
# admission takes no session lock. The admission tests, exactly identity.rs's
# (`identity::tests::`, `identity::invocation_tests::`) and this track's
# (`tests::session_overlap::`): in this process (a source whose reads meet
# in pairs, and this host's memory) and on PostgreSQL (two admissions whose
# reads meet after they read; and, the test holding the session's lock as a
# command does, an admission answered without waiting, a due write skipped
# and made once the lock is free, another reader's page undelayed); pages
# loading their runtime and modules while commands run, in three engines
# (e2e-hosts/second-host.spec.mjs, through playwright.hosts.config.mjs, the
# engines' hosts at PORT and above); and the mutation controls, which refuse
# to run without a database, so the PostgreSQL tests never pass by
# skipping. Refused without PW_FEED_DATABASE_URL, a throwaway database.
e14-session-overlap:
    @if [ -z "${PW_FEED_DATABASE_URL:-}" ]; then \
       echo "e14-session-overlap: refused, it needs PW_FEED_DATABASE_URL (a throwaway PostgreSQL database)"; \
       exit 1; \
     fi
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server
    @mkdir -p docs/evidence/E14
    @{ export PW_STORE_DATABASE_URL="${PW_STORE_DATABASE_URL:-$PW_FEED_DATABASE_URL}"; \
       echo "ADR-0389, amended 2026-11-02 - two requests of one session whose admissions overlap are each answered, every request is admitted, and an admission takes no session lock"; echo; \
       echo "produced by: just e14-session-overlap"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; \
       echo "postgres: $(psql "$PW_FEED_DATABASE_URL" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       echo "== admission, in this process and on PostgreSQL (identity.rs: identity::tests::, identity::invocation_tests::; tests/session_overlap.rs)"; echo; \
       cargo test --locked -p pw-dev-server --bin pw-dev-server -- identity::tests:: identity::invocation_tests:: tests::session_overlap:: 2>&1 | grep -E '^(test |test result)|panicked at'; \
       echo; echo "== pages loading their runtime and modules while commands run, in three engines (e2e-hosts/second-host.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test --config playwright.hosts.config.mjs --reporter=line -g "pages loading their runtime and modules"; \
       echo; echo "== mutation controls (scripts/session_overlap_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/session_overlap_mutations.py; \
     } > docs/evidence/E14/session-overlap.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/session-overlap.txt
