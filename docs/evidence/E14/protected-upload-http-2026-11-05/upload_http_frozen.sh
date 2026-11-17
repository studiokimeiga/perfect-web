#!/usr/bin/env bash
set -euo pipefail
source /home/haki/pw-setup/env.sh
cd /home/haki/code/pw-wt/data-path
export PW_TABLES_DATABASE_URL="${PW_STORE_DATABASE_URL:?throwaway PostgreSQL required}"
export CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=2
git diff --quiet HEAD
git rev-parse HEAD > /tmp/pw-protected-migration/upload-http-source.txt
for filter in uploads:: blob:: tests::upload_generations:: tests::tables:: tests::tables_pg:: tests::protected_migration:: tests::conformance::; do
    file_filter=${filter//:/_}
    cargo test --quiet --locked -p pw-dev-server --bin pw-dev-server "$filter" -- --format=pretty --test-threads=2 > "/tmp/pw-protected-migration/upload-http-frozen-${file_filter}.log" 2>&1
done
just preflight > /tmp/pw-protected-migration/upload-http-frozen-preflight.log 2>&1
python3 scripts/generic_upload_http_mutations.py > /tmp/pw-protected-migration/upload-http-frozen-strict.log 2>&1
git diff --quiet HEAD
git rev-parse HEAD >> /tmp/pw-protected-migration/upload-http-source.txt
