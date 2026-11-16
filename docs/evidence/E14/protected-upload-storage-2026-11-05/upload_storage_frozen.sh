#!/bin/bash
set -euo pipefail
source /home/haki/pw-setup/env.sh
cd /home/haki/code/pw-wt/data-path
export CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=2
export PW_TABLES_DATABASE_URL="${PW_STORE_DATABASE_URL:?throwaway PostgreSQL required}"
expected=20511c86822cd72b2716b0ac3e10f1847862bcfc
test "$(git rev-parse HEAD)" = "$expected"
test -z "$(git status --porcelain)"
log=/tmp/pw-protected-migration
git rev-parse HEAD > "$log/upload-storage-source.txt"
run() { local name="$1"; shift; echo "starting $name"; "$@" > "$log/upload-storage-frozen-$name.log" 2>&1; echo "$name passed"; }
run host cargo test --locked -p pw-host --features engine
run server cargo test --locked -p pw-dev-server --bin pw-dev-server -- blob:: uploads:: tests::protected_migration:: tests::tables:: tests::tables_pg::
run preflight just preflight
run controls python3 /mnt/c/Users/haki/Documents/Codex/2026-11-02/pleris-continuation/work/protected-migration/upload_storage_strict.py
test "$(git rev-parse HEAD)" = "$expected"
test -z "$(git status --porcelain)"
echo 'source guard: clean' >> "$log/upload-storage-source.txt"
echo 0 > "$log/upload-storage-frozen.exit"
