#!/bin/bash
set -euo pipefail
source /home/haki/pw-setup/env.sh
cd /home/haki/code/pw-wt/data-path
export CARGO_BUILD_JOBS=4 RUST_TEST_THREADS=2
expected=ae09ede16c71743ef6d22640639fd0e52681e3fc
test "$(git rev-parse HEAD)" = "$expected"
test -z "$(git status --porcelain)"
log=/tmp/pw-protected-migration
git rev-parse HEAD > "$log/upload-registry-memory-source.txt"
run() { local name="$1"; shift; echo "starting $name"; "$@" > "$log/upload-registry-memory-frozen-$name.log" 2>&1; echo "$name passed"; }
run host cargo test --locked -p pw-host --features engine
run memory cargo test --locked -p pw-table-memory
run preflight just preflight
run registry-controls python3 /mnt/c/Users/haki/Documents/Codex/2026-11-02/pleris-continuation/work/protected-migration/upload_registry_strict.py
run row-controls python3 /mnt/c/Users/haki/Documents/Codex/2026-11-02/pleris-continuation/work/protected-migration/upload_memory_strict.py
test "$(git rev-parse HEAD)" = "$expected"
test -z "$(git status --porcelain)"
echo 'source guard: clean' >> "$log/upload-registry-memory-source.txt"
echo 0 > "$log/upload-registry-memory-frozen.exit"
