#!/usr/bin/env bash
# **What a production host's memory comes to** (track deploy, ADR-XXXX).
#
#   deploy/host-memory.sh        (the host on HOST_PORT, 12955 where unset)
#
# kiokun's host, the release build, started as a deployment starts it on
# the repository's sample data (its search CSV past the in-memory bound, so
# its index is built into the cache) and asked a page and a search: its
# peak resident memory (the kernel's VmHWM) is said, and held under the
# share of a 2 GiB VM that deploy/vm/pleris.service's MemoryHigh= gives it,
# past which a start would be throttled. Most of the peak is Cranelift
# compiling the `kiokun.site.Word` component as the host starts.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${HOST_PORT:-12955}"
VM_KIB=$((2 * 1024 * 1024))
high="$(sed -n 's/^MemoryHigh=\([0-9][0-9]*\)%$/\1/p' "$ROOT/deploy/vm/pleris.service")"
if [ -z "$high" ]; then
  echo "FAIL: deploy/vm/pleris.service states no MemoryHigh= as a share of the VM"
  exit 1
fi
bound=$((VM_KIB * high / 100))

WORK="$(mktemp -d)"
HOST=""
trap 'if [ -n "$HOST" ]; then kill -KILL "$HOST" 2>/dev/null || true; fi; rm -rf "$WORK"' EXIT

cargo build --quiet --locked --release --manifest-path "$ROOT/Cargo.toml" -p pw-dev-server
if ! bash "$ROOT/deploy/build-dist.sh" kiokun "$WORK/dist" > "$WORK/build.log" 2>&1; then
  tail -20 "$WORK/build.log"
  echo "FAIL: kiokun's production build"
  exit 1
fi
mkdir -p "$WORK/kiokun"
cp -r "$ROOT/examples/kiokun/data/han-1char-3" "$WORK/kiokun/output_dictionary"
# The sample's rows, repeated past the 4 MiB under which an index is kept in
# memory, as tests/production_process.rs builds them.
python3 - "$ROOT/examples/kiokun/data/search-sample.csv" "$WORK/kiokun/output_search_index.csv" <<'PY'
import sys
head, rows = open(sys.argv[1], encoding="utf-8").read().split("\n", 1)
out = head + "\n"
while len(out.encode()) < 5 << 20:
    out += rows
open(sys.argv[2], "w", encoding="utf-8").write(out)
PY
key="$(od -An -tx1 -N32 /dev/urandom | tr -d ' \n')"
env -i PATH="$PATH" PW_DEPLOYMENT=production PW_ORIGIN=https://preview.example \
  PW_IDENTITY_KEY="$key" PW_IDENTITY=none PORT="$PORT" \
  KIOKUN_DATA="$WORK/kiokun/output_dictionary" PW_CACHE_DIR="$WORK/cache" \
  "$ROOT/target/release/pw-dev-server" "$WORK/dist" > "$WORK/host.log" 2>&1 &
HOST=$!
for _ in $(seq 1200); do
  if grep -q '"event":"listening"' "$WORK/host.log"; then
    break
  fi
  if ! kill -0 "$HOST" 2>/dev/null; then
    tail -5 "$WORK/host.log"
    echo "FAIL: the host ended as it started"
    exit 1
  fi
  sleep 0.1
done
if ! grep -q '"event":"listening"' "$WORK/host.log"; then
  echo "FAIL: the host did not listen within 120 s"
  exit 1
fi
page="$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: preview.example' "http://127.0.0.1:$PORT/word/%E4%BA%BA" || true)"
search="$(curl -s -o /dev/null -w '%{http_code}' -H 'Host: preview.example' "http://127.0.0.1:$PORT/search?q=hito" || true)"
peak="$(awk '/^VmHWM/{print $2}' "/proc/$HOST/status")"
now="$(awk '/^VmRSS/{print $2}' "/proc/$HOST/status")"
echo "host memory: kiokun's release host on the sample data, $(nproc) CPUs: peak $((peak / 1024)) MiB through its start, a page ($page) and a search ($search); $((now / 1024)) MiB after"
echo "host memory: pleris.service's MemoryHigh=$high% of a 2 GiB VM is $((bound / 1024)) MiB"
if [ "$page" != 200 ] || [ "$search" != 200 ]; then
  echo "FAIL: the host did not answer its page and its search"
  exit 1
fi
if [ "$peak" -ge "$bound" ]; then
  echo "FAIL: kiokun's host takes more than MemoryHigh= gives it on a 2 GiB VM"
  exit 1
fi
echo "host memory: OK"
