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
e14-generic-upload-http:
    @mkdir -p docs/evidence/E14/generic-upload-http
    @PW_GENERIC_UPLOAD_HTTP_EVIDENCE=docs/evidence/E14/generic-upload-http python3 scripts/generic_upload_http_mutations.py > docs/evidence/E14/generic-upload-http.txt
    @cat docs/evidence/E14/generic-upload-http.txt
