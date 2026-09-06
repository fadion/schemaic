#!/usr/bin/env bash
#
# Build a signed RPM repository out of a directory of .rpm files.
#
#     packaging/repo/build-rpm-repo.sh <packages-dir> <output-dir>
#
# The output directory becomes the repository root - the URL that goes in a
# .repo file's baseurl, with `repodata/` and `packages/` under it.
#
# Two separate signatures are involved and dnf checks them with two separate
# options, so both are produced here:
#
#   gpgcheck=1       the signature *inside each .rpm*, added by `rpmsign
#                    --addsign`. This is why the packages are re-signed here
#                    rather than copied through untouched.
#   repo_gpgcheck=1  a detached signature over repodata/repomd.xml, which is
#                    what makes the *index* tamper-evident rather than just the
#                    packages it names.
#
# The .rpm published on the GitHub Release is deliberately left unsigned - the
# release must not be able to fail on a missing key - so the copy here differs
# from it by exactly the signature header. Same payload, same version, and
# `dnf upgrade` will happily replace one with the other.
#
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: $0 <packages-dir> <output-dir>" >&2
    exit 2
fi

PACKAGES_DIR="$(cd "$1" && pwd)"
OUTPUT_DIR="$2"

: "${GPG_KEY_ID:?set GPG_KEY_ID to the signing key fingerprint or uid}"

command -v createrepo_c >/dev/null || { echo "createrepo_c not found (install createrepo-c)" >&2; exit 1; }
command -v rpmsign >/dev/null || { echo "rpmsign not found (install rpm)" >&2; exit 1; }
command -v gpg >/dev/null || { echo "gpg not found" >&2; exit 1; }

shopt -s nullglob
rpms=("${PACKAGES_DIR}"/*.rpm)
shopt -u nullglob
if [ "${#rpms[@]}" -eq 0 ]; then
    echo "no .rpm files in ${PACKAGES_DIR}" >&2
    exit 1
fi

rm -rf "$OUTPUT_DIR"
mkdir -p "${OUTPUT_DIR}/packages"
for rpm in "${rpms[@]}"; do
    cp "$rpm" "${OUTPUT_DIR}/packages/"
done
echo "[rpm] staged ${#rpms[@]} package(s)"

# rpmsign drives gpg through a macro rather than calling it with arguments, so
# the passphrase has to be threaded in through %_gpg_sign_cmd_extra_args.
# `--pinentry-mode loopback` is what stops gpg trying to open a terminal it
# does not have on a CI runner; the passphrase goes in through a file rather
# than the command line so it never appears in the process table.
#
# Passed as --define rather than written to a macro file: ~/.rpmmacros would
# silently overwrite a developer own macros when this is run locally, and
# --macros takes the *whole* search path, so naming one file there means
# knowing where the distribution keeps the rest.
sign_args=(--define "_gpg_name ${GPG_KEY_ID}")
if [ -n "${GPG_PASSPHRASE_FILE:-}" ]; then
    sign_args+=(--define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback --passphrase-file ${GPG_PASSPHRASE_FILE}")
else
    sign_args+=(--define "_gpg_sign_cmd_extra_args --batch --pinentry-mode loopback")
fi

rpmsign "${sign_args[@]}" --addsign "${OUTPUT_DIR}"/packages/*.rpm

# Not decoration: --addsign has reported success on some rpm versions when gpg
# declined, and an unsigned package in a gpgcheck=1 repository fails on the
# user machine at install time rather than here. A signed header renders as
# "RSA/SHA256, <date>, Key ID <id>" and an unsigned one as "(none)", so the key
# id is the thing to look for - and asking for all three tags covers rpm
# versions that fill different ones.
for rpm in "${OUTPUT_DIR}"/packages/*.rpm; do
    sig="$(rpm -qp --qf '%{RSAHEADER:pgpsig}|%{DSAHEADER:pgpsig}|%{SIGPGP:pgpsig}' "$rpm" 2>/dev/null || true)"
    case "$sig" in
        *"Key ID"*) ;;
        *)
            echo "$(basename "$rpm") is not signed after rpmsign --addsign (got: ${sig})" >&2
            exit 1
            ;;
    esac
done
echo "[rpm] signed with ${GPG_KEY_ID}"

# gz rather than the zstd createrepo_c now defaults to: the repository has to
# be readable by the oldest dnf/yum we claim to support, and RHEL 8 era clients
# cannot decompress zstd metadata. It costs a few hundred kilobytes.
createrepo_c --general-compress-type=gz "$OUTPUT_DIR"

gpg_args=(--batch --yes --local-user "$GPG_KEY_ID")
if [ -n "${GPG_PASSPHRASE_FILE:-}" ]; then
    gpg_args+=(--pinentry-mode loopback --passphrase-file "$GPG_PASSPHRASE_FILE")
fi
gpg "${gpg_args[@]}" --detach-sign --armor --output "${OUTPUT_DIR}/repodata/repomd.xml.asc" "${OUTPUT_DIR}/repodata/repomd.xml"

echo "[rpm] repository at ${OUTPUT_DIR}"
