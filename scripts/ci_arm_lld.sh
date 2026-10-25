#!/usr/bin/env bash
# Native ARM64 CI only. Neither wasm32 nor x64 changes linker.
set -euo pipefail
if [[ "$(uname -m)" != "aarch64" ]]; then
  echo "ARM linker setup refused on $(uname -m)" >&2
  exit 2
fi
: "$RUNNER_TEMP"
: "$GITHUB_ENV"

# Pinned Ubuntu 24.04 package from the 2026-10-24 controlled comparison.
sudo apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
  'lld=1:18.0-59~exp2'
version="$(ld.lld --version)"
case "$version" in
  "Ubuntu LLD 18.1.3 "*) ;;
  *) echo "Unexpected linker: $version" >&2; exit 2 ;;
esac

# Cargo target-specific linker, never a global Rust compiler flag.
wrapper="$RUNNER_TEMP/pleris-arm-lld-cc"
cat > "$wrapper" <<'EOF'
#!/bin/sh
set -eu
if [ -e "$RUNNER_TEMP/pleris-arm-lld-probe-active" ]; then
  printf 'invoked\n' >> "$RUNNER_TEMP/pleris-arm-lld-probe-used"
fi
exec cc -fuse-ld=lld "$@"
EOF
chmod 0755 "$wrapper"

# Verify that Cargo itself chooses our linker, and execute its output.
probe="$RUNNER_TEMP/pleris-arm-lld-probe"
mkdir -p "$probe/src"
cat > "$probe/Cargo.toml" <<'EOF'
[package]
name = "pleris-arm-lld-probe"
version = "0.0.0"
edition = "2024"
EOF
cat > "$probe/src/main.rs" <<'EOF'
fn main() { assert_eq!(1 + 1, 2); }
EOF
export CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER="$wrapper"
cargo generate-lockfile --offline --manifest-path "$probe/Cargo.toml"
touch "$RUNNER_TEMP/pleris-arm-lld-probe-active"

# Explicit-target positive control.
cargo build --offline --locked --manifest-path "$probe/Cargo.toml" \
  --target aarch64-unknown-linux-gnu
test -s "$RUNNER_TEMP/pleris-arm-lld-probe-used"
"$probe/target/aarch64-unknown-linux-gnu/debug/pleris-arm-lld-probe"

# Required native-control: real CI invokes Cargo WITHOUT --target.
# Reset the evidence so the explicit-target control cannot cause a false pass.
rm "$RUNNER_TEMP/pleris-arm-lld-probe-used"
CARGO_TARGET_DIR="$probe/native-target" cargo build --offline --locked \
  --manifest-path "$probe/Cargo.toml"
test -s "$RUNNER_TEMP/pleris-arm-lld-probe-used"
"$probe/native-target/debug/pleris-arm-lld-probe"
rm "$RUNNER_TEMP/pleris-arm-lld-probe-active"
printf 'CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=%s\n' "$wrapper" >> "$GITHUB_ENV"
echo "Cargo ARM native linker verified: $version"
