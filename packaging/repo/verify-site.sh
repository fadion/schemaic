#!/usr/bin/env bash
#
# Read a built site back the way apt and dnf would, before anything is
# published.
#
#     packaging/repo/verify-site.sh <site-dir>
#
# There is no unit test that can cover this. The failure it exists to catch is
# not a wrong answer from a function - it is metadata that disagrees with the
# packages beside it, which every tool involved reports as success right up
# until a user machine tries to install from it. So the check is the one a
# client performs: verify the signatures against the *published* public key,
# then confirm that every file the indexes name exists and hashes to what they
# claim. The index half is verify-site.py; the signature half is here, where
# the real tools are.
#
# Deliberately offline: it reads the directory and never the network, so it is
# the same check on a CI runner and on a developer machine.
#
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <site-dir>" >&2
    exit 2
fi

SITE="$(cd "$1" && pwd)"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
KEYRING="${SITE}/schemaic-archive-keyring.gpg"

command -v gpgv >/dev/null || { echo "gpgv not found" >&2; exit 1; }
command -v python3 >/dev/null || { echo "python3 not found" >&2; exit 1; }
[ -s "$KEYRING" ] || { echo "no published keyring at ${KEYRING}" >&2; exit 1; }

fail=0
check() {
    if "$@" >/dev/null 2>&1; then
        echo "  ok"
    else
        echo "  FAILED: $*"
        fail=1
    fi
}

# Against the keyring the site publishes, not the developer own keyring: the
# question is whether a client that trusts the published key can verify these,
# and a machine that happens to hold the secret key would answer yes either way.
echo "[verify] APT signatures"
check gpgv --keyring "$KEYRING" "${SITE}/deb/dists/stable/InRelease"
check gpgv --keyring "$KEYRING" "${SITE}/deb/dists/stable/Release.gpg" "${SITE}/deb/dists/stable/Release"

echo "[verify] RPM index signature"
check gpgv --keyring "$KEYRING" "${SITE}/rpm/repodata/repomd.xml.asc" "${SITE}/rpm/repodata/repomd.xml"

# Signature and payload digest of every package, against a throwaway rpm
# database holding only the published key - not the machine one, which on a
# runner is empty and on a developer machine is theirs.
if command -v rpmkeys >/dev/null; then
    echo "[verify] RPM package signatures"
    RPMDB="$(mktemp -d)"
    trap 'rm -rf "$RPMDB"' EXIT
    rpmkeys --dbpath "$RPMDB" --import "${SITE}/schemaic.asc"
    for pkg in "${SITE}"/rpm/packages/*.rpm; do
        printf '  %s\n' "$(basename "$pkg")"
        check rpmkeys --dbpath "$RPMDB" --checksig "$pkg"
    done
else
    echo "[verify] rpmkeys not available - skipping the package signature check"
    fail=1
fi

echo "[verify] index contents"
if ! python3 "${HERE}/verify-site.py" "$SITE"; then
    fail=1
fi

echo
if [ "$fail" -ne 0 ]; then
    echo "[verify] this site is NOT publishable"
    exit 1
fi
echo "[verify] verifies as a client would read it"
