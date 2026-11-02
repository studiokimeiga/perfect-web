#!/usr/bin/env bash
# **One Ubuntu VM made ready for a Pleris production host** (track
# `deploy`, ADR-XXXX, a production host). Run ON the VM, as root, from a
# copy of deploy/vm/ (deploy/vm/release.sh copies it and runs it the first
# time); running it again changes nothing that is already so.
#
#   bash setup.sh <domain> <program>          program: store | feed | kiokun
#
# Ubuntu 24.04 LTS (noble) or 26.04 LTS (resolute), x86_64. It installs:
#   - PostgreSQL 18 from the PostgreSQL project's repository (PGDG), the
#     version CI tests against (postgres:18.6), and a role and database
#     `pleris`, reached over the Unix socket by peer authentication: there is
#     no database password to keep;
#   - Caddy from its own stable repository, with deploy/vm/Caddyfile for the
#     domain: automatic HTTPS (Let's Encrypt), HTTP/2 and HTTP/3;
#   - the `pleris` system user, /etc/pleris/pleris.env (its settings) and
#     /etc/pleris/secrets (root's, mode 600: its identity key and metrics
#     token, made here once with `openssl rand`, and its database's address),
#     and deploy/vm/pleris.service, enabled; release.sh starts it;
#   - unattended security upgrades, and a firewall that lets in SSH, HTTP and
#     HTTPS (TCP and, for HTTP/3, UDP) alone.
# Each repository's steps are its own install page's (checked 2026-11-02):
# https://www.postgresql.org/download/linux/ubuntu/ and
# https://caddyserver.com/docs/install.
set -euo pipefail

DOMAIN="${1:?the domain the program is served at, e.g. preview.kiokun.com}"
PROGRAM="${2:?the program: store, feed or kiokun}"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
case "$PROGRAM" in store | feed | kiokun) ;; *) echo "no program named $PROGRAM" >&2; exit 2 ;; esac
[ "$(id -u)" = 0 ] || { echo "setup.sh runs as root" >&2; exit 1; }
[ "$(uname -m)" = x86_64 ] || { echo "setup.sh: this recipe's host is built for x86_64" >&2; exit 1; }
export DEBIAN_FRONTEND=noninteractive

echo "== packages"
apt-get update -q
apt-get install -y -q postgresql-common curl gnupg debian-keyring debian-archive-keyring \
  apt-transport-https unattended-upgrades ufw rsync acl
if [ ! -f /etc/apt/sources.list.d/pgdg.list ] && [ ! -f /etc/apt/sources.list.d/pgdg.sources ]; then
  /usr/share/postgresql-common/pgdg/apt.postgresql.org.sh -y
fi
if [ ! -f /etc/apt/sources.list.d/caddy-stable.list ]; then
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
    | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
  curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
    > /etc/apt/sources.list.d/caddy-stable.list
  chmod o+r /usr/share/keyrings/caddy-stable-archive-keyring.gpg /etc/apt/sources.list.d/caddy-stable.list
fi
apt-get update -q
apt-get install -y -q postgresql-18 caddy
dpkg-reconfigure -f noninteractive unattended-upgrades

echo "== the service's user"
id pleris >/dev/null 2>&1 \
  || useradd --system --home-dir /var/lib/pleris --no-create-home --shell /usr/sbin/nologin pleris
install -d -m 0755 /opt/pleris /opt/pleris/releases

echo "== its database"
if [ "$PROGRAM" != kiokun ]; then
  runuser -u postgres -- psql -v ON_ERROR_STOP=1 -qtAc \
    "SELECT 1 FROM pg_roles WHERE rolname = 'pleris'" | grep -q 1 \
    || runuser -u postgres -- psql -v ON_ERROR_STOP=1 -qc "CREATE ROLE pleris LOGIN"
  runuser -u postgres -- psql -v ON_ERROR_STOP=1 -qtAc \
    "SELECT 1 FROM pg_database WHERE datname = 'pleris'" | grep -q 1 \
    || runuser -u postgres -- createdb -O pleris pleris
fi

echo "== its settings and secrets"
install -d -m 0755 /etc/pleris
if [ ! -f /etc/pleris/pleris.env ]; then
  {
    echo "# The host's settings (nothing secret): deploy/vm/setup.sh's, then the operator's."
    echo "PW_ORIGIN=https://$DOMAIN"
    echo "PW_IDENTITY=none"
    echo "PW_BIND=127.0.0.1"
    echo "PORT=8080"
    echo "PW_CACHE_DIR=/var/cache/pleris"
    case "$PROGRAM" in
      kiokun) echo "KIOKUN_DATA=/srv/kiokun/output_dictionary" ;;
      feed) echo "PW_BLOB_DIR=/var/lib/pleris/blobs" ;;
    esac
  } > /etc/pleris/pleris.env
  chmod 0644 /etc/pleris/pleris.env
fi
if [ ! -f /etc/pleris/secrets ]; then
  (
    umask 077
    {
      echo "# The host's secrets: root's alone; systemd hands the service a copy it alone reads."
      echo "PW_IDENTITY_KEY=$(openssl rand -hex 32)"
      echo "PW_METRICS_TOKEN=$(openssl rand -hex 24)"
      case "$PROGRAM" in
        store) echo "PW_STORE_DATABASE_URL=postgresql://pleris@%2Fvar%2Frun%2Fpostgresql/pleris" ;;
        feed) echo "PW_FEED_DATABASE_URL=postgresql://pleris@%2Fvar%2Frun%2Fpostgresql/pleris" ;;
      esac
    } > /etc/pleris/secrets
  )
fi
chown root:root /etc/pleris/secrets
chmod 0600 /etc/pleris/secrets

echo "== Caddy's site"
sed "s/^PLERIS_DOMAIN /$DOMAIN /" "$HERE/Caddyfile" > /etc/caddy/Caddyfile.new
caddy validate --adapter caddyfile --config /etc/caddy/Caddyfile.new
mv /etc/caddy/Caddyfile.new /etc/caddy/Caddyfile
install -d -o caddy -g caddy -m 0755 /var/log/caddy
systemctl enable --now caddy
systemctl reload caddy

echo "== the service"
install -m 0644 "$HERE/pleris.service" /etc/systemd/system/pleris.service
systemctl daemon-reload
systemctl enable pleris

echo "== the firewall: SSH, HTTP, HTTPS"
ufw allow OpenSSH
ufw allow 80/tcp
ufw allow 443/tcp
ufw allow 443/udp
ufw --force enable

echo "setup.sh: $DOMAIN ready for $PROGRAM; deploy/vm/release.sh puts a release there"
