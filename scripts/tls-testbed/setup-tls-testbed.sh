#!/usr/bin/env bash
#
# Schemaic TLS test-bed — builds a local CA and a deliberately-broken server
# certificate set, then configures the local MariaDB/MySQL and PostgreSQL
# servers to serve TLS, so every verification mode can be developed against a
# real outcome. See README.md for the matrix.
#
#   sudo bash setup-tls-testbed.sh [options]
#
#   --pg-clientcert   also append a `hostssl ... cert` line to pg_hba.conf so
#                     the client-certificate path is testable on Postgres
#                     (pg_hba.conf is backed up first). Off by default because
#                     it edits a file the package manages.
#   --force           regenerate certificates that already exist.
#   --certs-only      build the certificates and stop — no root, no server
#                     config. Useful for inspecting what it would produce:
#                       SCHEMAIC_TLS_DIR=/tmp/tls bash setup-tls-testbed.sh --certs-only
#   --teardown        undo everything this script installed.
#
# Every path it touches is detected, and each detection can be overridden:
#
#   SCHEMAIC_TLS_DIR        where the CA and certificates live  (/etc/schemaic-tls)
#   SCHEMAIC_TLS_HOST       the name the certificates carry     (schemaic-tls.test)
#   SCHEMAIC_TLS_PASSWORD   password for the test DB users      (schemaic)
#   SCHEMAIC_PG_VERSION     PostgreSQL major version            (newest installed)
#   SCHEMAIC_MYSQL_CONFD    MariaDB/MySQL drop-in directory     (detected)
#   SCHEMAIC_TLS_WIN_DIR    where to copy the client files      (Windows profile, on WSL)
#
# A server it cannot find is skipped, not an error — one engine is enough to
# work on the connection form.

set -euo pipefail

CERT_DIR="${SCHEMAIC_TLS_DIR:-/etc/schemaic-tls}"
CA_DIR="$CERT_DIR/ca"
OTHER_DIR="$CERT_DIR/otherca"
TEST_HOST="${SCHEMAIC_TLS_HOST:-schemaic-tls.test}"
TEST_PASSWORD="${SCHEMAIC_TLS_PASSWORD:-schemaic}"
# The one database the test accounts may reach. They exist to prove a handshake
# happened, so `SELECT 1` inside a sandbox is the whole requirement — and the
# password above is printed in this directory's README.
TEST_DB="${SCHEMAIC_TLS_DB:-schemaic_tls_test}"

PG_CLIENTCERT=0
FORCE=0
CERTS_ONLY=0
TEARDOWN=0
for arg in "$@"; do
  case "$arg" in
    --pg-clientcert) PG_CLIENTCERT=1 ;;
    --force)         FORCE=1 ;;
    --certs-only)    CERTS_ONLY=1 ;;
    --teardown)      TEARDOWN=1 ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

say()  { printf '\n\033[1;36m==> %s\033[0m\n' "$*"; }
skip() { printf '    (skipped: %s)\n' "$*"; }

# ------------------------------------------------------------------- detection

# PostgreSQL: the newest configured cluster, if any.
detect_pg() {
  local ver="${SCHEMAIC_PG_VERSION:-}"
  if [ -z "$ver" ] && [ -d /etc/postgresql ]; then
    ver="$(find /etc/postgresql -mindepth 1 -maxdepth 1 -type d -printf '%f\n' 2>/dev/null | sort -n | tail -1)"
  fi
  [ -n "$ver" ] && [ -d "/etc/postgresql/$ver/main" ] || return 1
  PG_VER="$ver"
  PG_CONF_DIR="/etc/postgresql/$ver/main"
}

# MariaDB/MySQL: the drop-in directory the server actually includes, plus the
# service name and client binary, which differ between the two.
detect_mysql() {
  local d
  MY_CONFD="${SCHEMAIC_MYSQL_CONFD:-}"
  if [ -z "$MY_CONFD" ]; then
    for d in /etc/mysql/mariadb.conf.d /etc/mysql/mysql.conf.d /etc/mysql/conf.d; do
      [ -d "$d" ] && { MY_CONFD="$d"; break; }
    done
  fi
  [ -n "$MY_CONFD" ] && [ -d "$MY_CONFD" ] || return 1

  MY_SERVICE=""
  for d in mariadb mysql mysqld; do
    systemctl list-unit-files "$d.service" >/dev/null 2>&1 \
      && systemctl cat "$d.service" >/dev/null 2>&1 && { MY_SERVICE="$d"; break; }
  done
  [ -n "$MY_SERVICE" ] || return 1

  MY_CLIENT="$(command -v mariadb || command -v mysql || true)"
  [ -n "$MY_CLIENT" ] || return 1
  MY_SSL_DIR=/etc/mysql/ssl
}

# On WSL, the app runs on the Windows side and needs the CA file at a Windows
# path. Off WSL there is nothing to copy.
detect_win_dir() {
  if [ -n "${SCHEMAIC_TLS_WIN_DIR:-}" ]; then echo "$SCHEMAIC_TLS_WIN_DIR"; return; fi
  command -v cmd.exe >/dev/null 2>&1 || return
  command -v wslpath >/dev/null 2>&1 || return
  local profile
  profile="$( (cd /mnt/c 2>/dev/null || cd /); cmd.exe /c 'echo %USERPROFILE%' 2>/dev/null | tr -d '\r\n' )" || return
  case "$profile" in
    ?:\\*) wslpath -u "$profile" 2>/dev/null | sed 's|$|/schemaic-tls|' ;;
  esac
}

HAVE_PG=0; HAVE_MY=0
detect_pg    && HAVE_PG=1
detect_mysql && HAVE_MY=1

# --------------------------------------------------------------------- teardown

if [ "$TEARDOWN" = 1 ]; then
  [ "$(id -u)" -eq 0 ] || { echo "run me with sudo" >&2; exit 1; }
  say "Removing the test-bed"

  if [ "$HAVE_MY" = 1 ]; then
    rm -f "$MY_CONFD/99-schemaic-tls.cnf"
    rm -rf "$MY_SSL_DIR"
    # Put back whatever was in that directory before setup ran — it was
    # `rm -rf`d here with nothing said, while `pg_hba.conf` beside it was
    # carefully backed up and restored.
    if [ -d "$MY_SSL_DIR.pre-schemaic-tls" ]; then
      cp -a "$MY_SSL_DIR.pre-schemaic-tls" "$MY_SSL_DIR"
      rm -rf "$MY_SSL_DIR.pre-schemaic-tls"
      echo "  $MY_SSL_DIR restored from its backup"
    fi
    # The two accounts and their sandbox database. Leaving accounts behind is
    # not a tidiness question: they authenticate with a password this
    # directory's README prints.
    if [ -n "${MY_CLIENT:-}" ]; then
      "$MY_CLIENT" <<EOF || true
DROP USER IF EXISTS 'schemaic_ssl'@'localhost', 'schemaic_ssl'@'127.0.0.1', 'schemaic_ssl'@'%';
DROP USER IF EXISTS 'schemaic_x509'@'localhost', 'schemaic_x509'@'127.0.0.1', 'schemaic_x509'@'%';
DROP DATABASE IF EXISTS \`$TEST_DB\`;
FLUSH PRIVILEGES;
EOF
      echo "  dropped the schemaic_ssl / schemaic_x509 accounts and $TEST_DB"
    fi
    systemctl restart "$MY_SERVICE" && echo "  $MY_SERVICE back to plaintext"
  fi

  if [ "$HAVE_PG" = 1 ]; then
    rm -f "$PG_CONF_DIR/conf.d/99-schemaic-tls.conf"
    rm -f "$PG_CONF_DIR/schemaic-ca.crt" "$PG_CONF_DIR/schemaic-server.crt" "$PG_CONF_DIR/schemaic-server.key"
    if [ -f "$PG_CONF_DIR/pg_hba.conf.pre-schemaic-tls" ]; then
      cp -a "$PG_CONF_DIR/pg_hba.conf.pre-schemaic-tls" "$PG_CONF_DIR/pg_hba.conf"
      rm -f "$PG_CONF_DIR/pg_hba.conf.pre-schemaic-tls"
      echo "  pg_hba.conf restored from its backup"
    fi
    systemctl restart postgresql && echo "  postgresql back to its packaged certificate"
  fi

  # `$TEST_HOST` is interpolated into a regex, so a name carrying a `.` (every
  # name here does) matches more lines than it should — and a name carrying a
  # `/` would end the expression. Escaped, and anchored to the exact word.
  sed -i "/[[:space:]]$(printf '%s' "$TEST_HOST" | sed 's/[.[\*^$\/]/\\&/g')\$/d" /etc/hosts
  rm -rf "$CERT_DIR"
  echo "  removed $CERT_DIR and the /etc/hosts entry"

  # The Windows-profile copies, which include `client.key`. Setup writes them
  # and teardown used to leave them: a private key sitting in a user's home
  # directory long after the test-bed is gone.
  WIN_DIR="$(detect_win_dir || true)"
  if [ -n "$WIN_DIR" ] && [ -d "$WIN_DIR" ]; then
    rm -f "$WIN_DIR/ca.crt" "$WIN_DIR/otherca.crt" "$WIN_DIR/client.crt" "$WIN_DIR/client.key"
    rmdir "$WIN_DIR" 2>/dev/null || true
    echo "  removed the client copies in $WIN_DIR"
  fi
  echo
  echo "PostgreSQL's test roles are left in place; drop them by hand if you want them gone."
  exit 0
fi

if [ "$CERTS_ONLY" != 1 ] && [ "$(id -u)" -ne 0 ]; then
  echo "run me with sudo (or pass --certs-only)" >&2
  exit 1
fi

# ---------------------------------------------------------------- certificates

init_ca() {  # $1 = dir, $2 = CN
  local dir="$1" cn="$2"
  mkdir -p "$dir/newcerts"
  [ -f "$dir/index.txt" ] || : > "$dir/index.txt"
  [ -f "$dir/index.txt.attr" ] || echo "unique_subject = no" > "$dir/index.txt.attr"
  [ -f "$dir/serial" ] || echo 1000 > "$dir/serial"

  cat > "$dir/ca.cnf" <<EOF
[ ca ]
default_ca = CA_default

[ CA_default ]
dir             = $dir
database        = \$dir/index.txt
serial          = \$dir/serial
new_certs_dir   = \$dir/newcerts
certificate     = \$dir/ca.crt
private_key     = \$dir/ca.key
default_md      = sha256
default_days    = 3650
policy          = policy_any
email_in_dn     = no
rand_serial     = no
unique_subject  = no
copy_extensions = none

[ policy_any ]
commonName             = supplied
countryName            = optional
stateOrProvinceName    = optional
organizationName       = optional
organizationalUnitName = optional
emailAddress           = optional
EOF

  if [ ! -f "$dir/ca.crt" ] || [ "$FORCE" = 1 ]; then
    openssl req -x509 -newkey rsa:2048 -nodes -sha256 -days 3650 \
      -keyout "$dir/ca.key" -out "$dir/ca.crt" -subj "/CN=$cn" \
      -addext "basicConstraints=critical,CA:TRUE,pathlen:0" \
      -addext "keyUsage=critical,keyCertSign,cRLSign" 2>/dev/null
    echo "  CA: $dir/ca.crt  ($cn)"
  else
    echo "  CA: $dir/ca.crt  (kept)"
  fi
}

# issue <ca-dir> <name> <CN> <SAN-line> <EKU> [startdate enddate]
issue() {
  local ca="$1" name="$2" cn="$3" san="$4" eku="$5" start="${6:-}" end="${7:-}"
  local out="$CERT_DIR/$name"

  if [ -f "$out.crt" ] && [ "$FORCE" != 1 ]; then
    echo "  $name.crt (kept)"
    return
  fi

  {
    echo "basicConstraints = critical,CA:FALSE"
    echo "keyUsage = critical,digitalSignature,keyEncipherment"
    echo "extendedKeyUsage = $eku"
    echo "subjectKeyIdentifier = hash"
    echo "authorityKeyIdentifier = keyid,issuer"
    [ -n "$san" ] && echo "subjectAltName = $san"
  } > "$out.ext"

  openssl req -new -newkey rsa:2048 -nodes -sha256 \
    -keyout "$out.key" -out "$out.csr" -subj "/CN=$cn" 2>/dev/null

  local dates=(-days 3650)
  [ -n "$start" ] && dates=(-startdate "$start" -enddate "$end")

  openssl ca -config "$ca/ca.cnf" -batch -notext -md sha256 \
    -extfile "$out.ext" "${dates[@]}" \
    -in "$out.csr" -out "$out.crt" 2>/dev/null

  rm -f "$out.csr"
  echo "  $name.crt  ($cn)"
}

say "Generating the CA + certificate matrix in $CERT_DIR"
mkdir -p "$CERT_DIR"
chmod 755 "$CERT_DIR"
init_ca "$CA_DIR"    "Schemaic Test CA"
init_ca "$OTHER_DIR" "Schemaic Other CA (untrusted)"

GOOD_SAN="DNS:localhost,DNS:$TEST_HOST,IP:127.0.0.1"
EXP_START="$(date -u -d '-2 years' +%y%m%d%H%M%SZ)"
EXP_END="$(date -u -d '-1 year'  +%y%m%d%H%M%SZ)"

# 1. the happy path — every verification mode should accept this
issue "$CA_DIR"    server-good      "$TEST_HOST"      "$GOOD_SAN"         serverAuth
# 2. right CA, wrong name — verify-ca accepts, verify-full must reject.
#    This pair is the only test that can tell those two modes apart.
issue "$CA_DIR"    server-wrongname "wrong.example"   "DNS:wrong.example" serverAuth
# 3. right CA, right name, expired — every verifying mode must reject
issue "$CA_DIR"    server-expired   "$TEST_HOST"      "$GOOD_SAN"         serverAuth "$EXP_START" "$EXP_END"
# 4. valid certificate from a CA that is not in our trust file
issue "$OTHER_DIR" server-otherca   "$TEST_HOST"      "$GOOD_SAN"         serverAuth
# 5. client certificate for the mutual-TLS path. The CN doubles as the Postgres
#    role name, because `cert` auth matches the CN against the role.
issue "$CA_DIR"    client           "schemaic_client" ""                  clientAuth

cp -f "$CA_DIR/ca.crt"    "$CERT_DIR/ca.crt"
cp -f "$OTHER_DIR/ca.crt" "$CERT_DIR/otherca.crt"
chmod 644 "$CERT_DIR"/*.crt
# The client key stays 0600, like every other key here. It authenticates as
# schemaic_x509 and, with --pg-clientcert, as a PostgreSQL LOGIN role: a
# world-readable copy hands that to every local account. The Windows copies
# below are chowned to the invoking user for the same reason.

if [ "$CERTS_ONLY" = 1 ]; then
  say "Certificates only — servers untouched"
  echo "  would configure: MariaDB/MySQL=$([ "$HAVE_MY" = 1 ] && echo "${MY_CONFD:-} via $MY_SERVICE" || echo none)"
  echo "                   PostgreSQL=$([ "$HAVE_PG" = 1 ] && echo "${PG_CONF_DIR:-}" || echo none)"
  echo "                   client files=$(detect_win_dir || echo '(not WSL)')"
  echo
  for c in server-good server-wrongname server-expired server-otherca client; do
    printf '  %-17s %s\n' "$c" \
      "$(openssl x509 -noout -subject -issuer -dates -ext subjectAltName -in "$CERT_DIR/$c.crt" 2>/dev/null \
         | tr '\n' ' ' | sed 's/  */ /g')"
  done
  exit 0
fi

# --------------------------------------------------------------- MariaDB/MySQL

say "Configuring MariaDB/MySQL (TLS on, plaintext still allowed)"
if [ "$HAVE_MY" = 1 ]; then
  mkdir -p "$MY_SSL_DIR"
  # **Back up whatever was there first**, the way `pg_hba.conf` already is.
  # `install` below writes over `$MY_SSL_DIR/{ca,server}.{crt,key}` and teardown
  # `rm -rf`s the whole directory, so a machine that already had a certificate
  # there lost it with nothing said. One snapshot, kept next to the directory
  # and never overwritten by a second run.
  if [ ! -d "$MY_SSL_DIR.pre-schemaic-tls" ] && [ -n "$(ls -A "$MY_SSL_DIR" 2>/dev/null)" ]; then
    cp -a "$MY_SSL_DIR" "$MY_SSL_DIR.pre-schemaic-tls"
    echo "  backed up $MY_SSL_DIR to $MY_SSL_DIR.pre-schemaic-tls"
  fi
  install -o mysql -g mysql -m 644 "$CERT_DIR/ca.crt"          "$MY_SSL_DIR/ca.crt"
  install -o mysql -g mysql -m 644 "$CERT_DIR/server-good.crt" "$MY_SSL_DIR/server.crt"
  install -o mysql -g mysql -m 600 "$CERT_DIR/server-good.key" "$MY_SSL_DIR/server.key"

  # [mysqld] rather than [mariadbd]: mariadbd reads it too, so one file serves
  # both engines.
  cat > "$MY_CONFD/99-schemaic-tls.cnf" <<EOF
# Written by setup-tls-testbed.sh. Delete this file to go back to plaintext.
[mysqld]
ssl_ca   = $MY_SSL_DIR/ca.crt
ssl_cert = $MY_SSL_DIR/server.crt
ssl_key  = $MY_SSL_DIR/server.key
tls_version = TLSv1.2,TLSv1.3
# Left OFF on purpose: with plaintext still allowed, the disable/prefer modes
# stay testable. Turn it ON to test "server refuses an unencrypted connection".
#require_secure_transport = ON
EOF

  systemctl restart "$MY_SERVICE"
  echo "  have_ssl is now: $("$MY_CLIENT" -Ns -e "SHOW VARIABLES LIKE 'have_ssl'" | awk '{print $2}')"

  # **Scoped, and local.** These two accounts exist to prove a handshake
  # happened: they need to log in and run `SELECT 1`, and nothing more. They
  # used to be created at host `%` with GRANT ALL PRIVILEGES ON *.*, on a server
  # whose bind_address is 0.0.0.0, with a password this directory's README
  # prints — which reached every database on the machine, the real fixtures
  # included, from anywhere on the network.
  #
  # `localhost` is the socket and `127.0.0.1` is what the app dials; $TEST_HOST
  # resolves to the second.
  "$MY_CLIENT" <<EOF
CREATE DATABASE IF NOT EXISTS \`$TEST_DB\`;
CREATE USER IF NOT EXISTS 'schemaic_ssl'@'localhost'  IDENTIFIED BY '$TEST_PASSWORD' REQUIRE SSL;
CREATE USER IF NOT EXISTS 'schemaic_ssl'@'127.0.0.1'  IDENTIFIED BY '$TEST_PASSWORD' REQUIRE SSL;
CREATE USER IF NOT EXISTS 'schemaic_x509'@'localhost' IDENTIFIED BY '$TEST_PASSWORD' REQUIRE X509;
CREATE USER IF NOT EXISTS 'schemaic_x509'@'127.0.0.1' IDENTIFIED BY '$TEST_PASSWORD' REQUIRE X509;
GRANT ALL PRIVILEGES ON \`$TEST_DB\`.* TO 'schemaic_ssl'@'localhost';
GRANT ALL PRIVILEGES ON \`$TEST_DB\`.* TO 'schemaic_ssl'@'127.0.0.1';
GRANT ALL PRIVILEGES ON \`$TEST_DB\`.* TO 'schemaic_x509'@'localhost';
GRANT ALL PRIVILEGES ON \`$TEST_DB\`.* TO 'schemaic_x509'@'127.0.0.1';
FLUSH PRIVILEGES;
EOF
  echo "  database $TEST_DB — the only one these accounts can reach"
  echo "  user schemaic_ssl  (REQUIRE SSL)  — rejects a plaintext login"
  echo "  user schemaic_x509 (REQUIRE X509) — needs the client certificate"
else
  skip "no MariaDB/MySQL server found"
fi

# ------------------------------------------------------------------ PostgreSQL

say "Configuring PostgreSQL"
if [ "$HAVE_PG" = 1 ]; then
  install -o postgres -g postgres -m 644 "$CERT_DIR/ca.crt"          "$PG_CONF_DIR/schemaic-ca.crt"
  install -o postgres -g postgres -m 644 "$CERT_DIR/server-good.crt" "$PG_CONF_DIR/schemaic-server.crt"
  install -o postgres -g postgres -m 600 "$CERT_DIR/server-good.key" "$PG_CONF_DIR/schemaic-server.key"

  cat > "$PG_CONF_DIR/conf.d/99-schemaic-tls.conf" <<EOF
# Written by setup-tls-testbed.sh. Delete this file to fall back to the
# packaged certificate.
ssl = on
ssl_cert_file = '$PG_CONF_DIR/schemaic-server.crt'
ssl_key_file  = '$PG_CONF_DIR/schemaic-server.key'
ssl_ca_file   = '$PG_CONF_DIR/schemaic-ca.crt'
ssl_min_protocol_version = 'TLSv1.2'
EOF

  if [ "$PG_CLIENTCERT" = 1 ]; then
    HBA="$PG_CONF_DIR/pg_hba.conf"
    if ! grep -q schemaic_client "$HBA"; then
      cp -a "$HBA" "$HBA.pre-schemaic-tls"
      printf '\n# Added by setup-tls-testbed.sh - client-certificate auth test\nhostssl all schemaic_client 0.0.0.0/0 cert clientcert=verify-full\n' >> "$HBA"
      echo "  appended a hostssl/cert line (backup: $HBA.pre-schemaic-tls)"
    fi
    sudo -u postgres psql -qtc "SELECT 1 FROM pg_roles WHERE rolname = 'schemaic_client'" | grep -q 1 \
      || sudo -u postgres psql -qc "CREATE ROLE schemaic_client LOGIN SUPERUSER"
    echo "  role schemaic_client ready (authenticates by certificate CN)"
  fi

  systemctl reload postgresql || systemctl restart postgresql
  echo "  ssl is now: $(sudo -u postgres psql -Atc 'show ssl')"
else
  skip "no PostgreSQL cluster found"
fi

# ---------------------------------------------------- name, client files, state

say "Name resolution and client-side copies"
if ! grep -qE "[[:space:]]$TEST_HOST\$" /etc/hosts; then
  echo "127.0.0.1 $TEST_HOST" >> /etc/hosts
  echo "  added $TEST_HOST to /etc/hosts"
fi

WIN_DIR="$(detect_win_dir || true)"
if [ -n "$WIN_DIR" ]; then
  mkdir -p "$WIN_DIR"
  cp -f "$CERT_DIR/ca.crt" "$CERT_DIR/otherca.crt" "$CERT_DIR/client.crt" "$CERT_DIR/client.key" "$WIN_DIR/"
  chown -R "${SUDO_USER:-$USER}" "$WIN_DIR" 2>/dev/null || true
  echo "  client files for the Windows app: $WIN_DIR"
else
  skip "not WSL, or the Windows profile could not be resolved"
fi

# Recorded so swap-server-cert.sh does not have to detect any of this again.
cat > "$CERT_DIR/testbed.env" <<EOF
# Written by setup-tls-testbed.sh; read by swap-server-cert.sh.
CERT_DIR=$CERT_DIR
TEST_HOST=$TEST_HOST
HAVE_MY=$HAVE_MY
HAVE_PG=$HAVE_PG
MY_SSL_DIR=${MY_SSL_DIR:-}
MY_SERVICE=${MY_SERVICE:-}
PG_CONF_DIR=${PG_CONF_DIR:-}
EOF

cat <<EOF

Done.

If the app runs on Windows against a server in WSL, one manual step is left,
because Windows cannot be edited from here. As Administrator, add this to
C:\\Windows\\System32\\drivers\\etc\\hosts:

    127.0.0.1 $TEST_HOST

verify-full verifies a *name*, and that is the name the certificates carry.
WSL2 forwards localhost, so it resolves straight to these servers.

Check the server side before involving the app — README.md has the commands,
and swap-server-cert.sh walks the certificate matrix.
EOF
