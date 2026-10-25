# Track `upload-metadata`'s recipes (ADR-XXXX, amending ADR-0260): an
# uploaded image kept and served without what names a place, a device, a
# person or a time. The track adds its recipes here and nowhere else; the
# root justfile imports this file, and its settings and variables (`set
# shell`, `PATH`) apply here as there. An evidence recipe is `e14-<name>:`,
# writes `docs/evidence/E14/<name>.txt`, and is planned by the verification
# run like any other (scripts/ci_plan.py reads this file too).

# The rewrite of each format before an upload is kept (uploads/strip.rs:
# JPEG, PNG, WebP and GIF, each with what names a place, a device, a person
# or a time put in and none of it kept, what decoding, colour and a photo's
# turn need kept byte for byte, a file that does not parse refused, the
# rewrite idempotent, and any bytes refused or kept again unchanged; and
# uploads/icc.rs, an ICC profile kept for its colour alone), the
# upload path that leases, measures and keeps what it rewrote (uploads.rs),
# and the feed that serves it over HTTP (tests/uploads.rs): the server's
# `uploads` and `blob` tests. Then the browser suite in three engines
# (e2e/upload-metadata.spec.mjs: each format decoded in the feed at its
# markup's size and served without its metadata at its own key, a portrait
# photo upright, and EVIDENCE lines of what each engine shows of the files
# as sent); and the mutation controls (scripts/upload_metadata_mutations.py).
e14-upload-metadata:
    @BUILD_ONLY=1 bash spikes/own-renderer/run.sh > /dev/null
    @bash spikes/own-renderer/feed.sh > /dev/null
    @cargo build --quiet --locked -p pw-dev-server -p kiokun-server
    @mkdir -p docs/evidence/E14
    @{ echo "ADR-XXXX (track upload-metadata) - an uploaded image kept and served without what names a place, a device, a person or a time"; echo; \
       echo "produced by: just e14-upload-metadata"; \
       echo "commit: $(git rev-parse HEAD)$(git diff --quiet HEAD -- . ':(exclude)docs/evidence' ':(exclude)spikes/own-renderer/store-ir.json' || echo ' + uncommitted changes')"; \
       echo "rust: $(rustc --version)"; echo "node: $(node --version)"; \
       echo "playwright: $(cd spikes/own-renderer && pnpm exec playwright --version)"; echo; \
       echo "== the server (uploads/strip.rs, uploads/icc.rs, uploads.rs, tests/uploads.rs, blob.rs)"; echo; \
       cargo test --locked -p pw-dev-server -- uploads blob 2>&1 | awk '/^(test |test result)/ { print } /panicked at/ { p = 5 } p > 0 { print; p-- }'; \
       echo; echo "== an image shown and served without its metadata, in three engines (e2e/upload-metadata.spec.mjs)"; echo; \
       bash scripts/run_suite.sh -- pnpm exec playwright test e2e/upload-metadata.spec.mjs --reporter=line; \
       echo; echo "== mutation controls (scripts/upload_metadata_mutations.py)"; echo; \
       CARGO_INCREMENTAL=0 python3 scripts/upload_metadata_mutations.py; \
     } > docs/evidence/E14/upload-metadata.txt
    @grep -E "^test result|passed|mutants killed|^---- |panicked at|EVIDENCE" docs/evidence/E14/upload-metadata.txt
