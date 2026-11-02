#!/usr/bin/env bash
# The local production smoke test (track `deploy`, ADR-XXXX, a production
# host; `just e14-deploy` records it). The store, served as production
# serves it, on this machine:
#
#   - the release host (`cargo build --release`), PW_DEPLOYMENT=production,
#     started with nothing of this shell's environment but PATH, its secrets
#     where systemd's LoadCredential= hands them: a file `secrets` in
#     $CREDENTIALS_DIRECTORY, mode 0440 as systemd's ACL shows it;
#   - the store's production build (deploy/build-dist.sh: the runtime
#     minified);
#   - a database of its own on the local PostgreSQL, its migrations applied
#     at the host's start under their advisory lock;
#   - a TLS front with a certificate made for the run, Caddy's stand-in
#     (spikes/own-renderer/production/tls-front.mjs);
#   - the browsers (spikes/own-renderer/production/production.spec.mjs), in
#     each engine, before and after the host is stopped with SIGTERM and
#     started again.
#
# It says what it checks as it goes; any check that fails ends it, nonzero.
# PORT (14241 here) is the front's; the host listens on PORT+1.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT="${PORT:-14241}"
HOST_PORT=$((PORT + 1))
ORIGIN="https://localhost:$PORT"
ADMIN_URL="${PW_STORE_DATABASE_URL:?deploy/smoke.sh: PW_STORE_DATABASE_URL names the local PostgreSQL}"
DB="pw_deploy_smoke_$PORT"
DB_URL="${ADMIN_URL%/*}/$DB"
WORK="$(mktemp -d /tmp/pw-deploy-smoke.XXXXXX)"
HOST_PID=""
FRONT_PID=""

cleanup() {
  [ -n "$HOST_PID" ] && kill "$HOST_PID" 2>/dev/null || true
  [ -n "$FRONT_PID" ] && kill "$FRONT_PID" 2>/dev/null || true
  wait 2>/dev/null || true
  psql "$ADMIN_URL" -qc "DROP DATABASE IF EXISTS $DB WITH (FORCE)" >/dev/null 2>&1 || true
  chmod -R u+w "$WORK" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

fail() {
  echo "FAIL: $*"
  echo "-- the host's last lines:"
  tail -20 "$WORK/host.log" 2>/dev/null || true
  exit 1
}

echo "== 1. the release host and the store's production build"
cargo build --quiet --locked --release --manifest-path "$ROOT/Cargo.toml" -p pw-dev-server
HOST_BIN="$ROOT/target/release/pw-dev-server"
"$ROOT/deploy/build-dist.sh" store "$WORK/dist" | tail -1
node -e '
const info = JSON.parse(require("node:fs").readFileSync(process.argv[1], "utf8"));
const r = info.runtime;
console.log(`   runtime: ${r.written.bytes} B as written (${r.written.gzip} B gzipped), ` +
  `${r.served.bytes} B minified (${r.served.gzip} B gzipped)`);
' "$WORK/dist/build-info.json"
echo "   host: $(stat -c %s "$HOST_BIN") B ($HOST_BIN)"

echo
echo "== 2. its database, its secrets, its TLS"
psql "$ADMIN_URL" -qc "DROP DATABASE IF EXISTS $DB WITH (FORCE)" -c "CREATE DATABASE $DB" >/dev/null
echo "   database $DB on the local PostgreSQL ($(psql "$ADMIN_URL" -Atc 'SHOW server_version'))"
mkdir "$WORK/credentials"
(
  umask 077
  {
    echo "# the smoke's: made for this run, removed with it"
    echo "PW_IDENTITY_KEY=$(openssl rand -hex 32)"
    echo "PW_STORE_DATABASE_URL=$DB_URL"
    echo "PW_METRICS_TOKEN=$(openssl rand -hex 24)"
  } > "$WORK/credentials/secrets"
)
chmod 0440 "$WORK/credentials/secrets"
chmod 0500 "$WORK/credentials"
SECRETS="$WORK/credentials/secrets"
METRICS_TOKEN="$(sed -n 's/^PW_METRICS_TOKEN=//p' "$SECRETS")"
echo "   secrets: \$CREDENTIALS_DIRECTORY/secrets, mode $(stat -c %a "$SECRETS"), names: $(sed -n 's/=.*//p' "$SECRETS" | grep -v '^#' | tr '\n' ' ')"
openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 1 \
  -subj /CN=localhost -addext "subjectAltName=DNS:localhost" \
  -keyout "$WORK/tls.key" -out "$WORK/tls.crt" 2>/dev/null
node "$ROOT/spikes/own-renderer/production/tls-front.mjs" "$PORT" "$HOST_PORT" \
  "$WORK/tls.crt" "$WORK/tls.key" > "$WORK/front.log" 2>&1 &
FRONT_PID=$!
echo "   TLS front on $PORT (a certificate for localhost, made for this run), the host on $HOST_PORT"

start_host() {
  env -i PATH="$PATH" \
    PW_DEPLOYMENT=production PW_ORIGIN="$ORIGIN" PW_IDENTITY=none \
    CREDENTIALS_DIRECTORY="$WORK/credentials" PORT="$HOST_PORT" \
    "$HOST_BIN" "$WORK/dist" >> "$WORK/host.log" 2>&1 &
  HOST_PID=$!
  for _ in $(seq 1 300); do
    if curl -sk --max-time 2 "$ORIGIN/_pw/ready" 2>/dev/null | grep -q '"ready"'; then
      return 0
    fi
    kill -0 "$HOST_PID" 2>/dev/null || fail "the host ended at its start"
    sleep 0.2
  done
  fail "the host was not ready within a minute"
}

migrations() {
  psql "$DB_URL" -Atc "SELECT count(*) || ' applied, the last at ' || max(applied_at) FROM pw_store.pw_store_migrations"
}

echo
echo "== 3. the host, production"
start_host
echo "   ready at $ORIGIN/_pw/ready"
echo "   migrations: $(migrations)"
APPLIED_FIRST="$(migrations)"

echo
echo "== 4. its answers, through the front"
HEAD="$(curl -sk -D - -o /dev/null "$ORIGIN/stores/47")"
for field in "strict-transport-security: max-age=31536000" "x-content-type-options: nosniff" \
             "cache-control: private, no-store"; do
  grep -qi "^$field" <<< "$HEAD" || fail "the store's page has no \`$field\`"
  echo "   page: $field"
done
grep -qi "^content-security-policy: default-src 'none'" <<< "$HEAD" || fail "no policy"
echo "   page: content-security-policy: default-src 'none'; …"
grep -qi "^content-encoding" <<< "$HEAD" && fail "a session's page was compressed"
echo "   page: not compressed (a session's answer; BREACH)"
VERSION="$(curl -sk "$ORIGIN/stores/47" | sed -n 's/.*pw-runtime\.mjs?v=\([0-9a-f]*\).*/\1/p' | head -1)"
[ -n "$VERSION" ] || fail "the page does not start the runtime at a version"
echo "   page starts /pw-runtime.mjs?v=$VERSION"
ASSET="$(curl -sk -D - -o "$WORK/runtime.gz" -H 'Accept-Encoding: gzip' "$ORIGIN/pw-runtime.mjs?v=$VERSION")"
grep -qi '^content-encoding: gzip' <<< "$ASSET" || fail "the runtime is not compressed"
grep -qi '^cache-control: public, max-age=31536000, immutable' <<< "$ASSET" || fail "the runtime is not kept"
gunzip -c "$WORK/runtime.gz" | cmp -s - "$WORK/dist/pw-runtime.mjs" || fail "the gzip is not the runtime"
echo "   runtime: $(stat -c %s "$WORK/runtime.gz") B gzipped, immutable at its version, the bytes it holds the built runtime's"
for path in /bench/clock /menu-entry "/StorePage.html?items=40" /build-info.json /metrics /sign-in; do
  code="$(curl -sk -o /dev/null -w '%{http_code}' "$ORIGIN$path")"
  [ "$code" = 404 ] || fail "$path answered $code"
  echo "   $path: 404"
done
code="$(curl -sk -o /dev/null -w '%{http_code}' -H "Authorization: Bearer $METRICS_TOKEN" "$ORIGIN/metrics")"
[ "$code" = 200 ] || fail "/metrics with its token answered $code"
echo "   /metrics with its token: 200"

# The browsers' phase `$1`, in each engine: what each test did, and whether
# all passed.
browsers() {
  local status=0
  (cd "$ROOT/spikes/own-renderer" && PW_SMOKE_PHASE="$1" PORT="$PORT" \
    pnpm exec playwright test -c playwright.production.config.mjs) > "$WORK/$1.txt" 2>&1 || status=$?
  sed 's/\x1b\[[0-9;]*m//g' "$WORK/$1.txt" | grep -E '^ +(✓|✘|-) |^ +[0-9]+ (passed|failed|flaky|skipped)' \
    | sed 's/ ([0-9.]*m\?s)$//' || true
  if [ "$status" != 0 ]; then
    sed 's/\x1b\[[0-9;]*m//g' "$WORK/$1.txt" | grep -E -A12 '^ +[0-9]+\) ' | head -60
    fail "the browsers' phase $1"
  fi
}

echo
echo "== 5. the browsers, before a restart"
export PW_SMOKE_STATE="$WORK/state" PW_SMOKE_METRICS_TOKEN="$METRICS_TOKEN"
browsers first

echo
echo "== 6. a restart: SIGTERM, then a start on the same database"
STOP_AT=$(date +%s%N)
kill -TERM "$HOST_PID"
STATUS=0
wait "$HOST_PID" || STATUS=$?
HOST_PID=""
[ "$STATUS" = 0 ] || fail "the host ended with $STATUS on SIGTERM"
echo "   stopped with 0 in $(( ($(date +%s%N) - STOP_AT) / 1000000 )) ms"
start_host
APPLIED_AGAIN="$(migrations)"
[ "$APPLIED_FIRST" = "$APPLIED_AGAIN" ] || fail "the migrations changed: $APPLIED_FIRST, then $APPLIED_AGAIN"
echo "   migrations: $APPLIED_AGAIN (none applied again)"

echo
echo "== 7. the browsers, after the restart"
browsers again

echo
echo "== 8. its log: every line JSON, no secret in it"
kill -TERM "$HOST_PID"
wait "$HOST_PID" || true
HOST_PID=""
node -e '
const fs = require("node:fs");
const [log, secrets] = process.argv.slice(1);
const lines = fs.readFileSync(log, "utf8").split("\n").filter(Boolean);
const values = fs.readFileSync(secrets, "utf8").split("\n")
  .filter((l) => l && !l.startsWith("#")).map((l) => l.slice(l.indexOf("=") + 1));
const events = {};
for (const line of lines) {
  let read;
  try { read = JSON.parse(line); } catch { console.log(`FAIL: not a JSON line: ${line}`); process.exit(1); }
  for (const v of values) {
    if (line.includes(v)) { console.log(`FAIL: a secret in the log (${read.event})`); process.exit(1); }
  }
  events[read.event] = (events[read.event] ?? 0) + 1;
}
const errors = lines.map((l) => JSON.parse(l)).filter((l) => l.level === "error");
console.log(`   ${lines.length} lines, each JSON; no secret in any; events: ` +
  Object.entries(events).map(([e, n]) => `${e} ${n}`).join(", "));
if (errors.length) { console.log(`FAIL: ${errors.length} error line(s): ${JSON.stringify(errors[0])}`); process.exit(1); }
' "$WORK/host.log" "$SECRETS" || fail "the log"
grep '"event":"stopping"\|"event":"stopped"' "$WORK/host.log" | tail -2 | sed 's/^/   /'

echo
echo "deploy smoke: OK"
