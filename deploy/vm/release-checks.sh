#!/usr/bin/env bash
# **What deploy/vm/release.sh hands a machine, and what it refuses to**
# (track `deploy`, ADR-XXXX, a production host; the review's finding 3;
# `just e14-deploy` records it). Nothing here reaches a machine, or builds:
# `git`, `ssh`, `rsync` and `cargo` are stand-ins on PATH that write down
# how they were run. `git` says the tree is a clean commit where a case
# goes on (and ends a refused case's run at once, where the refusal did not
# come); `ssh` answers `uname -m` with x86_64 and ends anything else with 3,
# which stops release.sh there.
#
# Each refusal is a settings file as the owner's step makes one, mode 600,
# with one value release.sh would otherwise hand the VM: it must end with 2,
# by its reason, before any stand-in runs. The neighbour, every value well
# formed, goes on to `--setup`: its copy is root's on the VM (`rsync
# --chown=root:root --chmod=go-w`), and the VM's shell is handed the setup's
# command line as one argument, each value quoted for it.
#
# It prints one line a case and ends nonzero where any case is not as said.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
WORK="$(mktemp -d /tmp/pw-release-checks.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT
mkdir -p "$WORK/bin"
SAID="$WORK/said"
export SAID
cat > "$WORK/bin/git" <<'STANDIN'
#!/bin/sh
{ printf 'git'; for a in "$@"; do printf ' [%s]' "$a"; done; echo; } >> "$SAID"
[ "${GIT_CLEAN:-}" = 1 ] || exit 7
case "$*" in
  *rev-parse*) echo 1111111111111111111111111111111111111111 ;;
esac
exit 0
STANDIN
cat > "$WORK/bin/cargo" <<'STANDIN'
#!/bin/sh
{ printf 'cargo'; for a in "$@"; do printf ' [%s]' "$a"; done; echo; } >> "$SAID"
exit 7
STANDIN
cat > "$WORK/bin/ssh" <<'STANDIN'
#!/bin/sh
{ printf 'ssh'; for a in "$@"; do printf ' [%s]' "$a"; done; echo; } >> "$SAID"
case "$*" in
  *"uname -m") echo x86_64; exit 0 ;;
esac
exit 3
STANDIN
cat > "$WORK/bin/rsync" <<'STANDIN'
#!/bin/sh
{ printf 'rsync'; for a in "$@"; do printf ' [%s]' "$a"; done; echo; } >> "$SAID"
exit 0
STANDIN
chmod +x "$WORK/bin/git" "$WORK/bin/ssh" "$WORK/bin/rsync" "$WORK/bin/cargo"
FAILED=0

# release.sh with `program`, the settings given and `flags`, its tree clean
# where GIT_CLEAN=1; its exit and output, and what the stand-ins were asked.
run() {
  local program="$1" ssh="$2" key="$3" domain="$4"
  shift 4
  printf 'PLERIS_SSH=%q\nPLERIS_SSH_KEY=%q\nPLERIS_DOMAIN=%q\n' "$ssh" "$key" "$domain" > "$WORK/deploy.env"
  chmod 600 "$WORK/deploy.env"
  : > "$SAID"
  OUT="$(PATH="$WORK/bin:$PATH" PLERIS_DEPLOY_CONFIG="$WORK/deploy.env" \
    "$ROOT/deploy/vm/release.sh" "$program" "$@" 2>&1)"
  CODE=$?
  ASKED="$(cat "$SAID")"
}

refused() {
  local what="$1" want="$2"
  shift 2
  run "$@"
  if [ "$CODE" = 2 ] && grep -q -- "$want" <<< "$OUT" && [ -z "$ASKED" ]; then
    echo "   refused, exit 2, before any machine was asked: $what ($OUT)"
  else
    echo "FAIL: not refused before a machine was asked: $what (exit $CODE: $OUT; asked: $ASKED)"
    FAILED=1
  fi
}

KEY=/home/owner/.ssh/pleris_deploy_ed25519
refused "a program it does not build" "no program named" \
  'store;id' root@203.0.113.7 "$KEY" preview.example.com
refused "a domain the VM's shell would read as two commands" "not a host name" \
  store root@203.0.113.7 "$KEY" 'preview.example.com;touch /tmp/pwned'
refused "a domain with a space" "not a host name" \
  store root@203.0.113.7 "$KEY" 'preview.example.com touch'
refused "a destination ssh would read as an option" "not user@host" \
  store '-oProxyCommand=touch /tmp/pwned' "$KEY" preview.example.com
refused "a key path rsync's -e would split" "not a path" \
  store root@203.0.113.7 '/home/owner/my key' preview.example.com
refused "a key path ssh would read as an option" "not a path" \
  store root@203.0.113.7 '-oProxyCommand=x' preview.example.com

# The neighbour: well formed, it goes on to the VM and its setup.
export GIT_CLEAN=1
run kiokun root@203.0.113.7 "$KEY" preview.example.com --setup
unset GIT_CLEAN
copy="$(grep '^rsync ' <<< "$ASKED" || true)"
setup="$(grep 'setup.sh' <<< "$ASKED" | grep '^ssh ' || true)"
if [ "$CODE" = 3 ] \
  && grep -qF '[--chown=root:root] [--chmod=go-w]' <<< "$copy" \
  && grep -qF '[root@203.0.113.7:/root/pleris-setup/]' <<< "$copy" \
  && grep -qF '[root@203.0.113.7] [bash /root/pleris-setup/setup.sh preview.example.com kiokun ]' <<< "$setup"; then
  echo "   well formed, it went on: the setup copied as root's ($copy)"
  echo "   and its command line handed as one quoted argument ($setup)"
else
  echo "FAIL: well-formed settings were not handed on as said (exit $CODE: $OUT; asked: $ASKED)"
  FAILED=1
fi
[ "$FAILED" = 0 ] && echo "release checks: OK"
exit "$FAILED"
