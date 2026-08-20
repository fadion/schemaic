#!/usr/bin/env bash
#
# Build an RPM around an already-built Schemaic binary.
#
#     packaging/linux/build-rpm.sh <version> <binary> <output-dir> [rpm-arch]
#
# The spec compiles nothing. The binary is cross-built once by `cargo-zigbuild`
# against glibc 2.31 and then wrapped by both this and build-deb.sh, so the deb
# and the rpm contain byte-identical payloads and there is one build to keep
# working rather than three.
#
set -euo pipefail

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
    echo "usage: $0 <version> <binary> <output-dir> [rpm-arch]" >&2
    exit 2
fi

VERSION="${1#v}"
BINARY="$2"
OUTPUT_DIR="$3"
RPM_ARCH="${4:-x86_64}"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT

bash "${HERE}/stage-payload.sh" "$VERSION" "$BINARY" "${STAGE}/SOURCES/payload"

# `_topdir` under a scratch directory keeps the build out of ~/rpmbuild, so a
# CI runner and a developer's machine behave the same way.
mkdir -p "${STAGE}"/{BUILD,BUILDROOT,RPMS,SRPMS,SPECS}

rpmbuild -bb "${HERE}/schemaic.spec" \
    --define "_topdir ${STAGE}" \
    --define "_sourcedir ${STAGE}/SOURCES" \
    --define "schemaic_version ${VERSION}" \
    --define "dist %{nil}" \
    --define "_buildhost schemaic.build" \
    --target "${RPM_ARCH}"

mkdir -p "$OUTPUT_DIR"
BUILT="$(find "${STAGE}/RPMS" -name '*.rpm' -type f | head -n1)"
[ -n "$BUILT" ] || { echo "rpmbuild produced no package" >&2; exit 1; }
OUT="${OUTPUT_DIR}/$(basename "$BUILT")"
mv "$BUILT" "$OUT"
echo "$OUT"
