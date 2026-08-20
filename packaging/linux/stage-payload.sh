#!/usr/bin/env bash
#
# Lay out the files a Schemaic Linux package installs, under a given root.
#
#     packaging/linux/stage-payload.sh <version> <binary> <root>
#
# Shared by build-deb.sh and build-rpm.sh so the two formats cannot drift into
# shipping different files - the packaging metadata differs between them by
# necessity, the payload does not.
#
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "usage: $0 <version> <binary> <root>" >&2
    exit 2
fi

VERSION="${1#v}"
BINARY="$2"
ROOT="$3"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${HERE}/../.." && pwd)"
APP_ID="io.github.fadion.Schemaic"

for f in "$BINARY" \
    "${HERE}/${APP_ID}.desktop" \
    "${HERE}/${APP_ID}.metainfo.xml" \
    "${REPO}/assets/icon.png" \
    "${REPO}/LICENSE" \
    "${REPO}/THIRD-PARTY-NOTICES.md" \
    "${REPO}/crates/schemaic-ui/fonts/LICENSE.txt" \
    "${REPO}/licenses/Lucide-LICENSE.txt"; do
    [ -f "$f" ] || { echo "missing required file: $f" >&2; exit 1; }
done

mkdir -p "${ROOT}/usr/bin" \
    "${ROOT}/usr/share/applications" \
    "${ROOT}/usr/share/metainfo" \
    "${ROOT}/usr/share/icons/hicolor/512x512/apps" \
    "${ROOT}/usr/share/doc/schemaic"

install -m 0755 "$BINARY" "${ROOT}/usr/bin/schemaic"
install -m 0644 "${HERE}/${APP_ID}.desktop" "${ROOT}/usr/share/applications/${APP_ID}.desktop"

# assets/icon.png is 512x512, which is the hicolor directory it goes in. A
# mismatch here is not a build error - the icon simply never resolves, and the
# app shows a generic one.
install -m 0644 "${REPO}/assets/icon.png" \
    "${ROOT}/usr/share/icons/hicolor/512x512/apps/${APP_ID}.png"

# The checked-in metainfo carries whatever version was current when it was last
# touched; the package must state its own, or a software centre keeps offering
# an update to a version already installed.
sed -E "s|<release version=\"[^\"]*\" date=\"[^\"]*\"/>|<release version=\"${VERSION}\" date=\"$(date -u +%Y-%m-%d)\"/>|" \
    "${HERE}/${APP_ID}.metainfo.xml" > "${ROOT}/usr/share/metainfo/${APP_ID}.metainfo.xml"
chmod 0644 "${ROOT}/usr/share/metainfo/${APP_ID}.metainfo.xml"

install -m 0644 "${REPO}/LICENSE" "${ROOT}/usr/share/doc/schemaic/LICENSE"
install -m 0644 "${REPO}/THIRD-PARTY-NOTICES.md" "${ROOT}/usr/share/doc/schemaic/THIRD-PARTY-NOTICES.md"
install -m 0644 "${REPO}/README.md" "${ROOT}/usr/share/doc/schemaic/README.md"
install -m 0644 "${REPO}/crates/schemaic-ui/fonts/LICENSE.txt" \
    "${ROOT}/usr/share/doc/schemaic/IBMPlex-OFL.txt"
install -m 0644 "${REPO}/licenses/Lucide-LICENSE.txt" \
    "${ROOT}/usr/share/doc/schemaic/Lucide-LICENSE.txt"
