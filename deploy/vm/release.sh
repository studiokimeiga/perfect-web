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
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PROGRAM="${1:?the program: store, feed or kiokun}"
shift
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
SSH=(ssh -i "$PLERIS_SSH_KEY" -o IdentitiesOnly=yes -o BatchMode=yes "$PLERIS_SSH")
RSYNC_SSH="ssh -i $PLERIS_SSH_KEY -o IdentitiesOnly=yes -o BatchMode=yes"

# A release is a commit: what runs is what the repository holds.
if ! git -C "$ROOT" diff --quiet HEAD -- . || [ -n "$(git -C "$ROOT" status --porcelain --untracked-files=no)" ]; then
  echo "release.sh: the tree has uncommitted changes; a release is a commit" >&2
  exit 1
fi
COMMIT="$(git -C "$ROOT" rev-parse HEAD)"
[ "$("${SSH[@]}" uname -m)" = x86_64 ] || { echo "release.sh: the VM is not x86_64" >&2; exit 1; }

if [ -n "$SETUP" ]; then
  rsync -a -e "$RSYNC_SSH" "$ROOT/deploy/vm/" "$PLERIS_SSH:/root/pleris-setup/"
  "${SSH[@]}" bash /root/pleris-setup/setup.sh "$PLERIS_DOMAIN" "$PROGRAM"
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
rsync -a --delete -e "$RSYNC_SSH" "$STAGE/" "$PLERIS_SSH:/opt/pleris/releases/$COMMIT/"
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
