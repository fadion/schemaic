#!/usr/bin/env bash
#
# Build a signed APT repository out of a directory of .deb files.
#
#     packaging/repo/build-apt-repo.sh <packages-dir> <output-dir>
#
# The output directory becomes the archive root - the URL that goes in a
# sources.list line, with `dists/` and `pool/` directly under it.
#
# Signing is not optional. An unsigned APT repository can only be consumed by
# writing `[trusted=yes]` in the client's sources entry, which trains a user to
# turn off the check that makes a repository safer than a downloaded file in
# the first place. So this script fails without a key rather than emitting
# something that only works with verification disabled: set GPG_KEY_ID (and
# GPG_PASSPHRASE_FILE, if the key has a passphrase - it should).
#
# apt-ftparchive comes from apt-utils. Note that it is the only piece here that
# has to run from *inside* the archive root: the `Filename:` field it writes is
# relative to the working directory, and apt resolves it against the archive
# URL, so generating it from anywhere else silently produces paths no client
# can fetch.
#
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <packages-dir> <output-dir>" >&2
    exit 2
fi

PACKAGES_DIR="$(cd "$1" && pwd)"
OUTPUT_DIR="$2"

: "${GPG_KEY_ID:?set GPG_KEY_ID to the signing key fingerprint or uid}"

# Origin and Label are what an unattended-upgrades user writes in
# Allowed-Origins ("Schemaic:stable"), so they are part of the published
# interface and not free text to reword later.
ORIGIN="Schemaic"
LABEL="Schemaic"
SUITE="stable"
COMPONENT="main"
ARCH="amd64"

command -v apt-ftparchive >/dev/null || { echo "apt-ftparchive not found (install apt-utils)" >&2; exit 1; }
command -v gpg >/dev/null || { echo "gpg not found" >&2; exit 1; }

shopt -s nullglob
debs=("${PACKAGES_DIR}"/*.deb)
shopt -u nullglob
if [ "${#debs[@]}" -eq 0 ]; then
    echo "no .deb files in ${PACKAGES_DIR}" >&2
    exit 1
fi

rm -rf "$OUTPUT_DIR"
POOL="${OUTPUT_DIR}/pool/${COMPONENT}/s/schemaic"
DIST="${OUTPUT_DIR}/dists/${SUITE}"
BINDIR="${DIST}/${COMPONENT}/binary-${ARCH}"
mkdir -p "$POOL" "$BINDIR"

for deb in "${debs[@]}"; do
    cp "$deb" "$POOL/"
done
echo "[apt] pooled ${#debs[@]} package(s)"

# Written before the top-level Release below, which checksums every file under
# dists/ - a component Release added afterwards would be listed nowhere and
# apt would reject it as an unexpected file.
cat > "${BINDIR}/Release" <<EOF
Archive: ${SUITE}
Suite: ${SUITE}
Component: ${COMPONENT}
Origin: ${ORIGIN}
Label: ${LABEL}
Architecture: ${ARCH}
EOF

(
    cd "$OUTPUT_DIR"
    apt-ftparchive --arch "$ARCH" packages pool > "dists/${SUITE}/${COMPONENT}/binary-${ARCH}/Packages"
    gzip -9nkf "dists/${SUITE}/${COMPONENT}/binary-${ARCH}/Packages"
    if command -v xz >/dev/null; then
        xz -9kf "dists/${SUITE}/${COMPONENT}/binary-${ARCH}/Packages"
    fi

    # No Valid-Until on purpose. It would expire the repository on a fixed
    # clock rather than on a release, so a quiet month would break `apt update`
    # for everyone with nothing wrong and nothing to fix but a rebuild.
    apt-ftparchive \
        -o "APT::FTPArchive::Release::Origin=${ORIGIN}" \
        -o "APT::FTPArchive::Release::Label=${LABEL}" \
        -o "APT::FTPArchive::Release::Suite=${SUITE}" \
        -o "APT::FTPArchive::Release::Codename=${SUITE}" \
        -o "APT::FTPArchive::Release::Architectures=${ARCH}" \
        -o "APT::FTPArchive::Release::Components=${COMPONENT}" \
        -o "APT::FTPArchive::Release::Description=Schemaic - native SQL editor" \
        release "dists/${SUITE}" > "dists/${SUITE}/Release.tmp"
    mv "dists/${SUITE}/Release.tmp" "dists/${SUITE}/Release"
)

gpg_args=(--batch --yes --local-user "$GPG_KEY_ID")
if [ -n "${GPG_PASSPHRASE_FILE:-}" ]; then
    gpg_args+=(--pinentry-mode loopback --passphrase-file "$GPG_PASSPHRASE_FILE")
fi

# Both forms: InRelease is what every apt since 1.0 asks for first, Release.gpg
# is the detached fallback older clients and some mirrors still use.
gpg "${gpg_args[@]}" --clearsign --output "${DIST}/InRelease" "${DIST}/Release"
gpg "${gpg_args[@]}" --detach-sign --armor --output "${DIST}/Release.gpg" "${DIST}/Release"

echo "[apt] signed with ${GPG_KEY_ID}"
echo "[apt] repository at ${OUTPUT_DIR}"
