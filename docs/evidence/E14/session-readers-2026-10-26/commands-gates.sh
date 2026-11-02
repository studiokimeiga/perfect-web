set -euo pipefail
. /home/haki/pw-setup/env.sh
export PATH=/home/haki/code/perfect-web/.toolchain/prefix/bin:$PATH
export CARGO_TARGET_DIR=/home/haki/code/pw-wt/scheduler-replay-20261026/target
export PW_STORE_TEST_LAYER=memory
cd /home/haki/code/pw-wt/session-revocation-20261026
log=/mnt/c/Users/haki/Documents/Codex/2026-10-26/con/work
printf '%s\n' controls > "$log/reader-final-phase.txt"
python3 scripts/session_reader_mutations.py --report "$log/reader-controls-final.json" > "$log/reader-controls-final.txt" 2>&1
printf '%s\n' focused-native > "$log/reader-final-phase.txt"
cargo test --locked -p pw-dev-server --bin pw-dev-server -- identity:: tests::session_revocation:: --test-threads=1 > "$log/reader-focused-final.txt" 2>&1
printf '%s\n' producer-tests > "$log/reader-final-phase.txt"
python3 -m unittest scripts.tests.test_session_reader_controls scripts.tests.test_cargo_assertion_results scripts.tests.test_cargo_test_results > "$log/reader-producer-tests-final.txt" 2>&1
printf '%s\n' preflight-and-build > "$log/reader-final-phase.txt"
just preflight > "$log/reader-preflight-final.txt" 2>&1
cargo clippy --locked -p pw-dev-server --all-targets -- -D warnings >> "$log/reader-preflight-final.txt" 2>&1
cargo build --locked -p pw-dev-server >> "$log/reader-preflight-final.txt" 2>&1
git diff --check >> "$log/reader-preflight-final.txt" 2>&1
printf '%s\n' complete > "$log/reader-final-phase.txt"
