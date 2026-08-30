#!/usr/bin/env bash
#
# Swap which certificate the local servers present, so one server can walk the
# whole verification matrix.
#
#   sudo bash swap-server-cert.sh <good|wrongname|expired|otherca> [mysql|postgres|both]
#   bash swap-server-cert.sh status
#
# A server presents exactly one certificate, so this is how the negative cases
# get exercised: point Schemaic at the server, swap underneath it, reconnect.
#
# Paths come from the testbed.env that setup-tls-testbed.sh wrote; override the
# location with SCHEMAIC_TLS_DIR if you generated the certificates elsewhere.

set -euo pipefail

CERT_DIR="${SCHEMAIC_TLS_DIR:-/etc/schemaic-tls}"
ENV_FILE="$CERT_DIR/testbed.env"

[ -f "$ENV_FILE" ] || { echo "no test-bed at $CERT_DIR — run setup-tls-testbed.sh first" >&2; exit 1; }
# shellcheck source=/dev/null
. "$ENV_FILE"

# Report which of the matrix a live certificate file is, by fingerprint.
which_cert() {
  local target want name
  [ -f "$1" ] || { echo "(missing)"; return; }
  target="$(openssl x509 -noout -fingerprint -sha256 -in "$1" 2>/dev/null | cut -d= -f2)"
  for name in good wrongname expired otherca; do
    [ -f "$CERT_DIR/server-$name.crt" ] || continue
    want="$(openssl x509 -noout -fingerprint -sha256 -in "$CERT_DIR/server-$name.crt" | cut -d= -f2)"
    [ "$target" = "$want" ] && { echo "$name"; return; }
  done
  echo "(not from the test-bed)"
}

status() {
  [ "${HAVE_MY:-0}" = 1 ] && echo "MariaDB/MySQL presents: $(which_cert "$MY_SSL_DIR/server.crt")"
  [ "${HAVE_PG:-0}" = 1 ] && echo "PostgreSQL   presents: $(which_cert "$PG_CONF_DIR/schemaic-server.crt")"
  return 0
}

[ $# -ge 1 ] || { echo "usage: $0 <good|wrongname|expired|otherca|status> [mysql|postgres|both]" >&2; exit 2; }
if [ "$1" = status ]; then status; exit 0; fi

[ "$(id -u)" -eq 0 ] || { echo "run me with sudo" >&2; exit 1; }

WHICH="$1"
TARGET="${2:-both}"
SRC="$CERT_DIR/server-$WHICH"

[ -f "$SRC.crt" ] || { echo "no such certificate: $SRC.crt" >&2; exit 1; }
case "$TARGET" in
  mysql|postgres|both) ;;
  *) echo "unknown target: $TARGET (mysql|postgres|both)" >&2; exit 2 ;;
esac

if [ "${HAVE_MY:-0}" = 1 ] && { [ "$TARGET" = mysql ] || [ "$TARGET" = both ]; }; then
  install -o mysql -g mysql -m 644 "$SRC.crt" "$MY_SSL_DIR/server.crt"
  install -o mysql -g mysql -m 600 "$SRC.key" "$MY_SSL_DIR/server.key"
  systemctl restart "$MY_SERVICE"
  echo "MariaDB/MySQL now presents: $WHICH"
fi

if [ "${HAVE_PG:-0}" = 1 ] && { [ "$TARGET" = postgres ] || [ "$TARGET" = both ]; }; then
  install -o postgres -g postgres -m 644 "$SRC.crt" "$PG_CONF_DIR/schemaic-server.crt"
  install -o postgres -g postgres -m 600 "$SRC.key" "$PG_CONF_DIR/schemaic-server.key"
  # A reload is enough — Postgres re-reads the certificate files on SIGHUP.
  systemctl reload postgresql
  echo "PostgreSQL now presents: $WHICH"
fi
