#!/usr/bin/env bash
# TodoMVC's official Cypress suite, compatibility copy v1, against Pleris
# (track `todomvc`, W7): the acceptance of its first milestone.
#
# It serves `spikes/own-renderer/dist-todomvc` (built by `todomvc.sh`) on a
# host of its own, PORT+143 (the Playwright suite's are PORT+140 to PORT+142),
# and runs an explicitly identified two-patch execution copy. The seven
# originals remain upstream's blobs (`UPSTREAM.md` records both identities):
#
#     cypress run --env framework=page/todomvc.app.Todos~ui --config baseUrl=http://127.0.0.1:<port>
#
# The suite visits `baseUrl + '/' + framework`: the name is the page's own
# path on the host. (The root, `.`, was tried: Cypress asks for `/.`, which no
# host here serves as `/`.) The name is in none of the suite's tables.
#
# **It does not skip.** Where Cypress cannot run (no Xvfb, no binary, a build
# that is missing, a host that does not start) this exits 1 and says why, and
# so does a run that printed no results: the evidence recipe (`just
# e14-todomvc`) must never record a pass for a suite that did not run. The
# verdict is `scripts/todomvc_cypress_result.py`'s, read from what Cypress
# printed: this exits with it. (`pnpm exec cypress install`, in this
# directory, fetches Cypress's binary into its own cache; `UPSTREAM.md` says
# what.) Cypress starts Xvfb itself where no display is set, and the one WSLg
# or a desktop gives is left out of its environment, so the run is the same on
# a machine with none.
#
# **The diagnostic** (`DIAGNOSTIC=1`, not the acceptance): the same adapted
# files, run as an app the suite's own tables treat as keeping nothing in
# `localStorage` (`noLocalStorageCheck`: `puremvc`, which has no known issues
# either), reached through the address's fragment, which the page ignores:
#
#     cypress run --env framework=puremvc --config baseUrl=http://127.0.0.1:<port>/page/todomvc.app.Todos~ui#
#
# The acceptance's own name is in none of the tables: all 29 official tests,
# including persistence, must execute and pass. The separate diagnostic may
# explain a failure; it never substitutes for passing acceptance (ADR-0368).
#
# Cypress's output goes to $OUT (default /tmp/pw-todomvc-cypress.txt, or
# /tmp/pw-todomvc-cypress-diagnostic.txt), with the browser's own console
# (ELECTRON_ENABLE_LOGGING) retained as diagnostic context; the report goes
# to standard output and records the observed native outcomes.
#
# Environment: PORT (the base, default 3141), DIAGNOSTIC, OUT, PW_DEV_SERVER
# (the host's binary, default target/debug/pw-dev-server), TODOMVC_DIST (the
# built app: a name in spikes/own-renderer or a path, default dist-todomvc),
# XVFB_BIN (default Xvfb). The last three are for the tests of this script
# (scripts/tests/test_todomvc_cypress.py), which run it with a Cypress that
# prints what the test says.
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$HERE/../.." && pwd)"
# Where cargo writes its builds: CARGO_TARGET_DIR where it is set (a
# worktree's own target), else the repository's `target/`. A relative one
# is cargo's working directory's, which is this script's at its start.
TARGET_DIR="${CARGO_TARGET_DIR:-$REPO_ROOT/target}"
case "$TARGET_DIR" in /*) ;; *) TARGET_DIR="$PWD/$TARGET_DIR" ;; esac
SPIKE="$REPO_ROOT/spikes/own-renderer"
DIST_NAME="${TODOMVC_DIST:-dist-todomvc}"
case "$DIST_NAME" in
  /*) DIST="$DIST_NAME" ;;
  *) DIST="$SPIKE/$DIST_NAME" ;;
esac
SERVER="${PW_DEV_SERVER:-$TARGET_DIR/debug/pw-dev-server}"
XVFB="${XVFB_BIN:-Xvfb}"
PORT="$(( ${PORT:-3141} + 143 ))"
if [ "${DIAGNOSTIC:-}" = 1 ]; then
  MODE=diagnostic
  FRAMEWORK="puremvc"
  BASE="http://127.0.0.1:$PORT/page/todomvc.app.Todos~ui#"
  OUT="${OUT:-/tmp/pw-todomvc-cypress-diagnostic.txt}"
  echo "cypress: the DIAGNOSTIC run (framework $FRAMEWORK, which the suite takes to keep nothing in localStorage): not the acceptance"
else
  MODE=acceptance
  FRAMEWORK="page/todomvc.app.Todos~ui"
  BASE="http://127.0.0.1:$PORT"
  OUT="${OUT:-/tmp/pw-todomvc-cypress.txt}"
fi

# A suite that cannot run is a failure, never a skip.
cannot() {
  echo "run.sh: Cypress cannot run: $*"
  exit 1
}

cd "$HERE"

# Each vendored file is the upstream's, byte for byte (UPSTREAM.md).
python3 "$REPO_ROOT/scripts/todomvc_cypress_vendored.py"

command -v "$XVFB" > /dev/null 2>&1 \
  || cannot "Xvfb is not installed (Cypress needs a display and starts Xvfb itself; apt: xvfb)"
pnpm exec cypress verify > /dev/null 2>&1 \
  || cannot "its binary is not installed, or does not start (pnpm exec cypress install, in spikes/todomvc-cypress; UPSTREAM.md says what it fetches)"
echo "cypress: $(pnpm exec cypress --version | tr '\n' ' ')"
CYPRESS_PACKAGE="$(pnpm exec cypress version --component package)"
CYPRESS_BINARY="$(pnpm exec cypress version --component binary)"
[ -d "$DIST/build" ] || cannot "$DIST is not built (bash spikes/own-renderer/todomvc.sh)"
[ -x "$SERVER" ] || cannot "the host $SERVER is not built (cargo build -p pw-dev-server)"

# An existing listener must never satisfy this run's readiness check.
python3 - "$PORT" <<'PY' || cannot "the host port is already occupied"
import socket, sys
with socket.socket() as held:
    held.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    held.bind(("127.0.0.1", int(sys.argv[1])))
PY

# A unique project below HERE resolves the installed pinned Cypress package.
# Its deterministic manifest is checked again immediately before execution.
EXECUTION="$(mktemp -d "$HERE/.execution-v1.XXXXXX")"
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

# The host: TodoMVC's build, in memory, on a port of its own.
(cd "$SPIKE" && exec env PORT="$PORT" PW_FEED_DATABASE_URL= PW_STORE_DATABASE_URL= \
  "$SERVER" "$DIST" > "$OUT.host.log" 2>&1) &
HOST=$!
for _ in $(seq 1 100); do
  kill -0 "$HOST" 2>/dev/null || cannot "the owned host exited before readiness"
  if curl -fs -o /dev/null "http://127.0.0.1:$PORT/"; then break; fi
  sleep 0.2
done
curl -fs -o /dev/null "http://127.0.0.1:$PORT/" \
  || cannot "the host did not start ($OUT.host.log)"
kill -0 "$HOST" 2>/dev/null || cannot "the owned host exited before Cypress launch"

python3 "$REPO_ROOT/scripts/todomvc_cypress_compat.py" verify "$EXECUTION/project" \
  --package-version "$CYPRESS_PACKAGE" --binary-version "$CYPRESS_BINARY"

set +e
env -u DISPLAY -u WAYLAND_DISPLAY ELECTRON_ENABLE_LOGGING=1 \
  pnpm exec cypress run \
  --env framework="$FRAMEWORK" \
  --config "baseUrl=$BASE" \
  --reporter spec --project "$EXECUTION/project" > "$OUT" 2>&1
CODE=$?
set -e

# The verdict: this exits with it. A run with no results table, or with an
# incomplete official acceptance suite, fails here.
python3 "$REPO_ROOT/scripts/todomvc_cypress_result.py" "$MODE" "$OUT" "$CODE"
