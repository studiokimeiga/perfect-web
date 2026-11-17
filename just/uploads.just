# The uploads track's recipes (ADR-0253, docs/PARALLEL.md): a typed upload,
# a deployment's blob storage, and serving it safely. The track adds its
# recipes here and nowhere else; the root justfile imports this file, and its
# settings and variables (`set shell`, `PATH`) apply here as there. An
# evidence recipe is `e14-<name>:`, writes `docs/evidence/E14/<name>.txt`,
# and is planned by the verification run like any other (scripts/ci_plan.py
# reads this file too).

# Track `uploads` (ADR-0260): an image on a post. The header readers, the
# form's body, the blob storage, the leases and serving (the server's
# `uploads` and `blob` tests, PostgreSQL's where PW_FEED_DATABASE_URL names
# a database and passing doing nothing otherwise); the `upload` declaration
# and its checks (compiler/pw-core/tests/uploads.rs); the browser suite in
# three engines (e2e/uploads.spec.mjs); and the mutation controls
# (scripts/uploads_mutations.py).
e14-uploads:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-0260 (track uploads) - an image on a post: a typed upload, a deployment's blob storage, served safely"; echo; \
       echo "produced by: just e14-uploads"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; \
       echo "postgres: $([ -n "${PW_FEED_DATABASE_URL:-}" ] && (psql "$PW_FEED_DATABASE_URL" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)') || echo 'none: PW_FEED_DATABASE_URL is not set')"; echo; \
       echo "== the server (uploads.rs, uploads/sniff.rs, uploads/multipart.rs, blob.rs, tests/uploads.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- uploads blob 2>&1 | awk '/^(test |test result)/ { print } /panicked at/ { p = 5 } p > 0 { print; p-- }'; \
       echo; echo "== the declaration and its checks (compiler/pw-core/tests/uploads.rs)"; echo; \
       cargo test --locked -p pw-core --test uploads 2>&1 | awk '/^(test |test result)/ { print } /panicked at/ { p = 5 } p > 0 { print; p-- }'; \
       echo; echo "== an image on a post in three engines (e2e/uploads.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/uploads.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/uploads_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/uploads_mutations.py; \
     } > docs/evidence/E14/uploads.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at" docs/evidence/E14/uploads.txt

# Generic authority + actual PG/local files + handler TCP composition. Requires
# the actual throwaway PW_TABLES_DATABASE_URL; includes strict native controls.
#
# It reads its database's rows and serves their files (PostgreSQL's layer,
# the local blob root, the host's HTTP): it needs a throwaway database,
# PW_TABLES_DATABASE_URL (or PW_STORE_DATABASE_URL, which CI's database
# shards name), and is refused without one. Until 2026-11-17 its recipe named
# none, so the verify dealt it where there was none and it failed at its
# script's first line (L5a's verify 43860567064).
e14-generic-upload-http:
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     if [ -z "$url" ]; then \
       echo "e14-generic-upload-http: refused, it needs PW_TABLES_DATABASE_URL (or PW_STORE_DATABASE_URL), a throwaway PostgreSQL database"; \
       exit 1; \
     fi
    @mkdir -p docs/evidence/E14/generic-upload-http
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     export PW_TABLES_DATABASE_URL="$url"; \
     { echo "generic-upload-http - a generic upload's image served from its database's rows and the blob root, each authority and file check held by a mutant"; echo; \
       echo "produced by: just e14-generic-upload-http"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; \
       echo "postgres: $(psql "$url" -Atc 'SHOW server_version' 2>/dev/null || echo 'unknown (psql is not on PATH)')"; echo; \
       PW_GENERIC_UPLOAD_HTTP_EVIDENCE=docs/evidence/E14/generic-upload-http python3 scripts/generic_upload_http_mutations.py; \
     } > docs/evidence/E14/generic-upload-http.txt 2>&1; \
     status=$?; cat docs/evidence/E14/generic-upload-http.txt; exit $status

# Generic per-declaration staging, captured-reader scope and lease lifetimes.
e14-generic-upload-leases:
    @mkdir -p docs/evidence/E14/generic-upload-leases
    @{ echo "generic-upload-leases - a generic upload's staging, its leases per session and declaration, each held by a mutant"; echo; \
       echo "produced by: just e14-generic-upload-leases"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       PW_GENERIC_UPLOAD_LEASE_EVIDENCE=docs/evidence/E14/generic-upload-leases python3 scripts/generic_upload_leases_mutations.py; \
     } > docs/evidence/E14/generic-upload-leases.txt 2>&1; \
     status=$?; cat docs/evidence/E14/generic-upload-leases.txt; exit $status

# The server's claim bridge (ADR-0440, L5b): a compiled command claims,
# publishes and commits a declared upload through the server, on memory and
# on PostgreSQL, each rule held by a mutant. It needs a throwaway database,
# PW_TABLES_DATABASE_URL (or PW_STORE_DATABASE_URL, which CI's database
# shards name): refused without one.
e14-generic-upload-claims:
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     if [ -z "$url" ]; then \
       echo "e14-generic-upload-claims: refused, it needs PW_TABLES_DATABASE_URL (or PW_STORE_DATABASE_URL), a throwaway PostgreSQL database"; \
       exit 1; \
     fi
    @mkdir -p docs/evidence/E14
    @url="${PW_TABLES_DATABASE_URL:-${PW_STORE_DATABASE_URL:-${PW_FEED_DATABASE_URL:-}}}"; \
     export PW_TABLES_DATABASE_URL="$url"; \
     { echo "generic-upload-claims - the server's claim bridge: a compiled command claims, publishes and commits a declared upload, each outcome settling its claims once"; echo; \
       echo "produced by: just e14-generic-upload-claims"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/generic_upload_claims_mutations.py; \
     } > docs/evidence/E14/generic-upload-claims.txt 2>&1; \
     status=$?; cat docs/evidence/E14/generic-upload-claims.txt; exit $status
