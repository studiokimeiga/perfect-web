#!/bin/bash
set -euo pipefail
source /home/haki/pw-setup/env.sh
cd /home/haki/code/pw-wt/data-path
export PW_TABLES_DATABASE_URL="${PW_STORE_DATABASE_URL:?throwaway PostgreSQL required}"
export CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=2
test "$(git rev-parse HEAD)" = 15f3c9df5542fea913f84896aabc841c94ce9369
test -z "$(git status --porcelain)"
git rev-parse HEAD > /tmp/pw-protected-migration/counts-verified-source.txt
# The affected compiler suites. The cancelled first checkpoint's ALL-core run
# has no complete-suite credit; full CI remains a final application-port gate.
cargo test --locked -p pw-core --lib --test tables --test table_counts --test table_owners --test bounded_reads --test bounded_tables --test invoked_by --test scheduled > /tmp/pw-protected-migration/counts-verified-core.log 2>&1
cargo test --locked -p pw-host --features engine > /tmp/pw-protected-migration/counts-verified-host.log 2>&1
cargo test --locked -p pw-table-memory > /tmp/pw-protected-migration/counts-verified-memory.log 2>&1
cargo test --locked -p pw-table-postgres > /tmp/pw-protected-migration/counts-verified-pg.log 2>&1
cargo test --locked -p pw-dev-server --bin pw-dev-server -- tests::protected_migration:: identity:: tests::store_accounts:: tests::sign_in:: tests::tables:: tests::conformance:: tests::tables_pg:: tests::second_host:: > /tmp/pw-protected-migration/counts-verified-server.log 2>&1
just test-compile > /tmp/pw-protected-migration/counts-verified-test-compile.log 2>&1
just preflight > /tmp/pw-protected-migration/counts-verified-preflight.log 2>&1
python3 scripts/d3_postgres_mutations.py counts > /tmp/pw-protected-migration/counts-verified-pg-controls.log 2>&1
python3 scripts/row_ownership_mutations.py counts > /tmp/pw-protected-migration/counts-verified-row-controls.log 2>&1
python3 scripts/data_path_mutations.py "count aggregate" > /tmp/pw-protected-migration/counts-verified-data-controls.log 2>&1
test "$(git rev-parse HEAD)" = 15f3c9df5542fea913f84896aabc841c94ce9369
test -z "$(git status --porcelain)"
echo 'source guard: clean' >> /tmp/pw-protected-migration/counts-verified-source.txt
echo 0 > /tmp/pw-protected-migration/counts-verified.exit
