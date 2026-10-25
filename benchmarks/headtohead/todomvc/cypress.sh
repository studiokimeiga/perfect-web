#!/usr/bin/env bash
# TodoMVC's official Cypress suite against TodoMVC's own React or Svelte app
# (the head-to-head, ADR-XXXX): the same compatibility copy TodoMVC in Pleris
# is held to (`todomvc-cypress-compat-v1`, spikes/todomvc-cypress/UPSTREAM.md),
# the same Cypress (15.21.1), on the vendored production build, served by the
# head-to-head's static host as the suite's own server lays the examples out:
#
#     cypress run --env framework=<react|svelte> --config baseUrl=http://127.0.0.1:<port>
#
# The suite visits `baseUrl + '/' + <react/dist|svelte/dist>` (its
# `frameworkFolders`); the host redirects the folder to its slash and serves
# its `index.html`, as `serve-static` does. Ports: PORT + 230 (react) and
# PORT + 231 (svelte), in the head-to-head's range.
#
# The verdict is `scripts/head_to_head_cypress.py`'s, read from what Cypress
# printed, and this exits with it: every test passes but those the suite's
# own tables skip for the app (its gaps, printed), or it fails. Where Cypress
# cannot run (no Xvfb, no binary), this exits 1 and says why: a suite that did
# not run is not a pass.
#
# Environment: PORT (the base, default 3141), OUT (Cypress's output, default
# /tmp/h2h-cypress-<app>.txt).
set -euo pipefail

APP="${1:?usage: cypress.sh <react|svelte>}"
HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/../../.." && pwd)"
SUITE="$REPO_ROOT/spikes/todomvc-cypress"
case "$APP" in
  react) PORT_APP="$(( ${PORT:-3141} + 230 ))" ;;
  svelte) PORT_APP="$(( ${PORT:-3141} + 231 ))" ;;
  *) echo "cypress.sh: no app $APP (react or svelte)"; exit 2 ;;
esac
OUT="${OUT:-/tmp/h2h-cypress-$APP.txt}"
DIST="$HERE/$APP/dist"

cannot() {
  echo "cypress.sh: Cypress cannot run: $*"
  exit 1
}

# The suite's seven files are upstream's, byte for byte.
python3 "$REPO_ROOT/scripts/todomvc_cypress_vendored.py"
command -v Xvfb > /dev/null 2>&1 || cannot "Xvfb is not installed (apt: xvfb)"
cd "$SUITE"
pnpm exec cypress verify > /dev/null 2>&1 \
  || cannot "its binary is not installed (pnpm exec cypress install, in spikes/todomvc-cypress)"
CYPRESS_PACKAGE="$(pnpm exec cypress version --component package)"
CYPRESS_BINARY="$(pnpm exec cypress version --component binary)"
[ -f "$DIST/index.html" ] || cannot "$DIST has no index.html"

python3 - "$PORT_APP" <<'PY' || cannot "port $PORT_APP is already taken"
import socket, sys
with socket.socket() as held:
    held.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    held.bind(("127.0.0.1", int(sys.argv[1])))
PY

EXECUTION="$(mktemp -d "$SUITE/.execution-v1.XXXXXX")"
HOST=
cleanup() {
  if [ -n "$HOST" ]; then
    kill "$HOST" 2>/dev/null || true
    wait "$HOST" 2>/dev/null || true
  fi
  rm -rf -- "$EXECUTION"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
python3 "$REPO_ROOT/scripts/todomvc_cypress_compat.py" create "$EXECUTION/project" \
  --package-version "$CYPRESS_PACKAGE" --binary-version "$CYPRESS_BINARY"

node "$REPO_ROOT/benchmarks/headtohead/static-host.mjs" --port "$PORT_APP" \
  --mount "/$APP/dist=$DIST" > "$OUT.host.log" 2>&1 &
HOST=$!
for _ in $(seq 1 100); do
  kill -0 "$HOST" 2>/dev/null || cannot "the static host exited before readiness"
  if curl -fs -o /dev/null "http://127.0.0.1:$PORT_APP/$APP/dist/"; then break; fi
  sleep 0.1
done
curl -fs -o /dev/null "http://127.0.0.1:$PORT_APP/$APP/dist/" || cannot "the static host did not start"

python3 "$REPO_ROOT/scripts/todomvc_cypress_compat.py" verify "$EXECUTION/project" \
  --package-version "$CYPRESS_PACKAGE" --binary-version "$CYPRESS_BINARY"

echo "cypress: $CYPRESS_PACKAGE (binary $CYPRESS_BINARY), todomvc-cypress-compat-v1, framework=$APP"
set +e
env -u DISPLAY -u WAYLAND_DISPLAY pnpm exec cypress run \
  --env framework="$APP" \
  --config "baseUrl=http://127.0.0.1:$PORT_APP" \
  --reporter spec --project "$EXECUTION/project" > "$OUT" 2>&1
CODE=$?
set -e

python3 "$REPO_ROOT/scripts/head_to_head_cypress.py" "$APP" "$OUT" "$CODE"
