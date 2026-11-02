#!/usr/bin/env bash
# **No production test host outlives its run** (track deploy, ADR-XXXX).
#
#   deploy/hosts-left.sh DIR
#   deploy/hosts-left.sh --self-check
#
# DIR is the TMPDIR a run of tests/production_process.rs was given, or that
# the deploy campaign (scripts/deploy_mutations.py) was, whose runs start
# under a directory of its own beneath it: each host such a run starts
# serves `DIR/.../pw-production-*`. A host still running a few seconds after
# its run ended is said, as a `FAIL:` line, killed, and makes the exit 1;
# none is said too. Only hosts under DIR are looked at, so a check never
# touches another run's, or another checkout's.
#
# `--self-check` holds the check itself: a stand-in host under one
# directory is found and killed, and another's, under another, is left.
#
# The tests' `Host` kills and reaps each host when its test is done with it,
# and the kernel kills it with a test process killed outright, so a host
# found here is a test that leaks: on 2026-11-02 two dozen leaked hosts, up
# to a gigabyte each, took the development machine's memory.
set -euo pipefail

# Each pid of a host under DIR. `[p]` keeps the pattern from matching a
# command line that holds it.
hosts_under() { pgrep -f -- "pw-dev-server ${1%/}/([^ ]*/)?[p]w-production-" || true; }

# Says, and kills, each host under DIR that a few seconds do not end;
# returns 1 where there was one.
check() {
  local dir="${1%/}" pids pid
  for _ in $(seq 50); do
    if [ -z "$(hosts_under "$dir")" ]; then
      echo "hosts left: none under $dir"
      return 0
    fi
    sleep 0.1
  done
  pids="$(hosts_under "$dir")"
  for pid in $pids; do
    echo "FAIL: a production test host outlived its run, killed: pid $pid: $(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)"
  done
  # shellcheck disable=SC2086 # each word a pid
  kill -KILL $pids 2>/dev/null || true
  return 1
}

if [ "${1:-}" = "--self-check" ]; then
  mine="$(mktemp -d)"
  theirs="$(mktemp -d)"
  # A stand-in host: a sleep whose command line is a host's under the
  # directory it is given.
  # shellcheck disable=SC2016 # $0 is the inner shell's
  bash -c 'exec -a "pw-dev-server $0/pw-production-stand-in" sleep 60' "$mine" &
  ours=$!
  # shellcheck disable=SC2016
  bash -c 'exec -a "pw-dev-server $0/pw-production-stand-in" sleep 60' "$theirs" &
  other=$!
  # Not this shell's jobs: a kill is not said as one.
  disown "$ours" "$other"
  trap 'kill -KILL "$ours" "$other" 2>/dev/null || true; rm -rf "$mine" "$theirs"' EXIT
  for _ in $(seq 50); do
    if [ -n "$(hosts_under "$mine")" ] && [ -n "$(hosts_under "$theirs")" ]; then
      break
    fi
    sleep 0.1
  done
  if check "$mine" > /dev/null; then
    echo "FAIL: hosts-left.sh found no host under its directory"
    exit 1
  fi
  if ! check "$mine" > /dev/null; then
    echo "FAIL: hosts-left.sh did not kill the host it found"
    exit 1
  fi
  if [ -z "$(hosts_under "$theirs")" ]; then
    echo "FAIL: hosts-left.sh touched a host under another directory"
    exit 1
  fi
  echo "hosts left: the check finds and kills a host under its directory, and leaves another's"
  exit 0
fi

check "${1:?usage: deploy/hosts-left.sh DIR (the TMPDIR a test run was given) | --self-check}"
