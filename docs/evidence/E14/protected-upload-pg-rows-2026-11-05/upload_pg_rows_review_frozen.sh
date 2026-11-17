#!/usr/bin/env bash
set -euo pipefail
source /home/haki/pw-setup/env.sh
cd /home/haki/code/pw-wt/data-path
export PW_TABLES_DATABASE_URL="${PW_STORE_DATABASE_URL:?throwaway PostgreSQL required}"
export CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=2
cargo test --quiet --locked -p pw-table-postgres -- --format=pretty --test-threads=2 > /tmp/pw-protected-migration/upload-pg-rows-review-all.log 2>&1
just preflight > /tmp/pw-protected-migration/upload-pg-rows-review-preflight.log 2>&1
python3 /mnt/c/Users/haki/Documents/Codex/2026-11-02/pleris-continuation/work/protected-migration/upload_pg_rows_strict.py > /tmp/pw-protected-migration/upload-pg-rows-review-strict.log 2>&1
