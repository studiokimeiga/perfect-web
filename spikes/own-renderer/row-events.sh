#!/usr/bin/env bash
# The large list's fixture of presses (`examples/rows/events.pw`), a build of
# its own for the browser suite, into `dist-row-events` (ROW_EVENTS_PORTS):
# the large list's build is the app alone, as the head-to-head measures it,
# and so a page alone in its build (ADR-0426).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
PW_OUT="${PW_OUT:-$HERE/dist-row-events}" ROWS_PAGE=events exec bash "$HERE/rows.sh"
