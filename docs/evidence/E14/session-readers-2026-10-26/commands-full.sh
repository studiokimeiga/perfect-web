set -euo pipefail
. /home/haki/pw-setup/env.sh
export PATH=/home/haki/code/perfect-web/.toolchain/prefix/bin:$PATH
export CARGO_TARGET_DIR=/home/haki/code/pw-wt/scheduler-replay-20261026/target
export PW_STORE_TEST_LAYER=memory
cd /home/haki/code/pw-wt/session-revocation-20261026
cargo test --locked -p pw-dev-server -- --test-threads=4
