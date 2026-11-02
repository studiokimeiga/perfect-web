#!/usr/bin/env bash
# **One release of a Pleris program to the VM** (track `deploy`, ADR-XXXX, a
# production host): built here from a clean commit, copied, switched to,
# restarted, and checked; put back if it is not ready.
#
#   deploy/vm/release.sh <program> [--setup] [--data]
#
#   --setup  first copy deploy/vm/ and run setup.sh on the VM (the first time)
#   --data   kiokun's: copy the dictionary and its search index to
#            /srv/kiokun (from KIOKUN_SOURCE, ~/code/kiokun-data by default)
#
# Nothing here reaches a VM without the owner's step (DEPLOY-OWNER-STEP.md
# in the handoff folder): its settings are the file
# ~/.config/pleris/deploy.env, mode 600, outside the repository:
#   PLERIS_SSH=root@<the VM's address>
#   PLERIS_SSH_KEY=$HOME/.ssh/pleris_deploy_ed25519
#   PLERIS_DOMAIN=preview.kiokun.com
#
# **A release is rolled back only over a schema it reads** (the review's
# finding 5k, recorded): a host applies the migrations it knows, each once,
# and refuses none it does not know, so the release before, put back on the
# database this release migrated, starts and says it is ready whatever its
# queries then meet. A migration a release adds is therefore one the release
# before it reads past (a new table; a new column with a default; a new
# index), and a change or removal of what an older release reads waits for a
# release after the one that stops reading it (expand, then contract). A
# release whose migration is not so is rolled forward, never back: its
# put-back below is then the operator's to refuse.
#
# Every value this script hands the VM is checked here first (the review's
# finding 3): the program against the three it knows, the domain as a host
# name, the destination and the key as names that hold no option and no
# space; and an argument the VM's shell reads is quoted for it (`printf %q`),
# never joined into a command line it would read again.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROGRAM="${1:?the program: store, feed or kiokun}"
shift
case "$PROGRAM" in
  store | feed | kiokun) ;;
  *) echo "release.sh: no program named \`$PROGRAM\`: store, feed or kiokun" >&2; exit 2 ;;
esac
SETUP=""
DATA=""
for flag in "$@"; do
  case "$flag" in
    --setup) SETUP=1 ;;
    --data) DATA=1 ;;
    *) echo "release.sh: no flag $flag" >&2; exit 2 ;;
  esac
done
CONFIG="${PLERIS_DEPLOY_CONFIG:-$HOME/.config/pleris/deploy.env}"
if [ ! -f "$CONFIG" ]; then
  echo "release.sh: no $CONFIG: the owner's step makes it (DEPLOY-OWNER-STEP.md)" >&2
  exit 1
fi
if [ "$(stat -c %a "$CONFIG")" != 600 ]; then
  echo "release.sh: $CONFIG is not mode 600" >&2
  exit 1
fi
# shellcheck disable=SC1090
. "$CONFIG"
: "${PLERIS_SSH:?$CONFIG names PLERIS_SSH}" "${PLERIS_SSH_KEY:?$CONFIG names PLERIS_SSH_KEY}"
: "${PLERIS_DOMAIN:?$CONFIG names PLERIS_DOMAIN}"
# A host name: labels of letters, digits and hyphens, joined by dots.
LABEL='[A-Za-z0-9]([A-Za-z0-9-]{0,61}[A-Za-z0-9])?'
if ! [[ "$PLERIS_DOMAIN" =~ ^$LABEL(\.$LABEL)+$ ]]; then
  echo "release.sh: PLERIS_DOMAIN is not a host name" >&2
  exit 2
fi
# user@host, which ssh reads as a destination and never as an option.
if ! [[ "$PLERIS_SSH" =~ ^[A-Za-z0-9._][A-Za-z0-9._-]*@[A-Za-z0-9.:_-]+$ ]]; then
  echo "release.sh: PLERIS_SSH is not user@host" >&2
  exit 2
fi
# A path with no space and no option in it: rsync's -e splits on spaces.
if ! [[ "$PLERIS_SSH_KEY" =~ ^[A-Za-z0-9._/~+-]+$ ]] || [[ "$PLERIS_SSH_KEY" == -* ]]; then
  echo "release.sh: PLERIS_SSH_KEY is not a path of letters, digits and ./_~+-" >&2
  exit 2
fi
SSH=(ssh -i "$PLERIS_SSH_KEY" -o IdentitiesOnly=yes -o BatchMode=yes "$PLERIS_SSH")
RSYNC_SSH="ssh -i $PLERIS_SSH_KEY -o IdentitiesOnly=yes -o BatchMode=yes"
# What it copies is root's on the VM, and no one else may change it: never
# the uid of the account that built it (rsync -a keeps it where the
# receiver is root), which could then replace the binary systemd runs.
RSYNC=(rsync -a --chown=root:root --chmod=go-w -e "$RSYNC_SSH")

# A release is a commit: what runs is what the repository holds.
if ! git -C "$ROOT" diff --quiet HEAD -- . || [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no)" ]; then
  echo "release.sh: the tree has uncommitted changes; a release is a commit" >&2
  exit 1
fi
COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
[[ "$COMMIT" =~ ^[0-9a-f]{40}$ ]] || { echo "release.sh: HEAD is no commit" >&2; exit 1; }
[ "$("${SSH[@]}" uname -m)" = x86_64 ] || { echo "release.sh: the VM is not x86_64" >&2; exit 1; }

if [ -n "$SETUP" ]; then
  "${RSYNC[@]}" "$ROOT/deploy/vm/" "$PLERIS_SSH:/root/pleris-setup/"
  # ssh hands the VM one command line, which its shell reads again: each
  # argument quoted for that shell, checked above besides.
  "${SSH[@]}" "bash /root/pleris-setup/setup.sh $(printf '%q ' "$PLERIS_DOMAIN" "$PROGRAM")"
fi
if [ -n "$DATA" ]; then
  [ "$PROGRAM" = kiokun ] || { echo "release.sh: --data is kiokun's" >&2; exit 2; }
  SOURCE="${KIOKUN_SOURCE:-$HOME/code/kiokun-data}"
  "${SSH[@]}" install -d -m 0755 /srv/kiokun
  # 1.49 million small files: one tar stream, not one transfer each.
  tar -C "$SOURCE" -cf - output_dictionary output_search_index.csv \
    | "${SSH[@]}" tar -C /srv/kiokun -xf - --no-same-owner
  "${SSH[@]}" chmod -R a+rX /srv/kiokun
fi

echo "== build: the release host and $PROGRAM's production build, at $COMMIT"
cargo build --quiet --locked --release --manifest-path "$ROOT/Cargo.toml" -p pw-dev-server
STAGE="$ROOT/target/deploy/$PROGRAM/release"
rm -rf "$STAGE"
mkdir -p "$STAGE"
"$ROOT/deploy/build-dist.sh" "$PROGRAM" "$STAGE/dist"
install -m 0755 "$ROOT/target/release/pw-dev-server" "$STAGE/pleris-host"

echo "== copy, switch, restart"
"${RSYNC[@]}" --delete "$STAGE/" "$PLERIS_SSH:/opt/pleris/releases/$COMMIT/"
"${SSH[@]}" bash -s -- "$COMMIT" <<'REMOTE'
set -euo pipefail
commit="$1"
previous="$(readlink /opt/pleris/current 2>/dev/null || true)"
echo "$previous" > /opt/pleris/previous
ln -sfn "/opt/pleris/releases/$commit" /opt/pleris/current.new
mv -T /opt/pleris/current.new /opt/pleris/current
systemctl restart pleris
REMOTE

echo "== ready?"
for _ in $(seq 1 60); do
  if curl -sf --max-time 3 "https://$PLERIS_DOMAIN/_pw/ready" | grep -q '"ready"'; then
    echo "release.sh: $PROGRAM at $COMMIT is ready at https://$PLERIS_DOMAIN"
    # The three newest releases are kept: the one before is a rollback.
    "${SSH[@]}" 'cd /opt/pleris/releases && ls -1t | tail -n +4 | xargs -r rm -rf'
    exit 0
  fi
  sleep 1
done
echo "release.sh: not ready within a minute; the release before is put back" >&2
"${SSH[@]}" 'p="$(cat /opt/pleris/previous)"; [ -n "$p" ] && ln -sfn "$p" /opt/pleris/current && systemctl restart pleris; journalctl -u pleris -n 30 --no-pager'
exit 1
