#!/usr/bin/env bash
#
# Build a Debian package around an already-built Schemaic binary.
#
#     packaging/linux/build-deb.sh <version> <binary> <output-dir> [deb-arch]
#
# Deliberately not `cargo-deb`. The binary this wraps is cross-built by
# `cargo-zigbuild` against glibc 2.31 so it runs on Debian 11 / Ubuntu 20.04 and
# newer, and cargo-deb's `depends = "$auto"` resolves shared libraries through
# `dpkg-shlibdeps`, which reads the *build host's* package versions - on a
# ubuntu-latest runner that would stamp the package with Ubuntu 24.04 minimums
# and refuse to install on precisely the distributions the zigbuild target
# exists to reach. So the dependency list below is written by hand, and the
# rules for changing it are in the comment above it.
#
set -euo pipefail

if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
    echo "usage: $0 <version> <binary> <output-dir> [deb-arch]" >&2
    exit 2
fi

VERSION="${1#v}"
BINARY="$2"
OUTPUT_DIR="$3"
DEB_ARCH="${4:-amd64}"

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "${HERE}/../.." && pwd)"

# Where a bug report should go. Not a personal address on purpose - this string
# ships inside every package that is ever downloaded and is not retractable
# afterwards. Change it here if a real contact address is wanted.
MAINTAINER="Schemaic contributors <fadion@users.noreply.github.com>"

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
ROOT="${STAGE}/schemaic"

bash "${HERE}/stage-payload.sh" "$VERSION" "$BINARY" "$ROOT"
mkdir -p "${ROOT}/DEBIAN"

# Debian's machine-readable copyright file. The bundled font and icon set have
# their own terms and are named here rather than folded into the MIT blanket.
# `sed` turns the LICENSE into the indented, dot-for-blank-line form the format
# requires.
{
    cat <<'EOF'
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: schemaic
Source: https://github.com/fadion/schemaic

Files: *
Copyright: 2026 Schemaic contributors
License: MIT

Files: usr/share/doc/schemaic/IBMPlex-OFL.txt
Copyright: 2017 IBM Corp.
License: OFL-1.1
 The IBM Plex fonts are licensed under the SIL Open Font License 1.1; the full
 text is in /usr/share/doc/schemaic/IBMPlex-OFL.txt.

Files: usr/share/doc/schemaic/Lucide-LICENSE.txt
Copyright: 2022 Lucide Contributors
License: ISC
 The Lucide icon set is licensed under the ISC license; the full text is in
 /usr/share/doc/schemaic/Lucide-LICENSE.txt.

License: MIT
EOF
    sed 's/^$/./; s/^/ /' "${REPO}/LICENSE"
} > "${ROOT}/usr/share/doc/schemaic/copyright"
chmod 0644 "${ROOT}/usr/share/doc/schemaic/copyright"

# Every one of these is loaded with dlopen, not linked: `readelf -d` on the
# binary lists glibc and nothing else, because winit reaches X11, Wayland and
# xkbcommon through libloading and wgpu reaches Vulkan and EGL the same way.
# That is exactly why the list is written out rather than derived - no
# dependency scanner, Debian's or rpm's, can see a library the loader only asks
# for at runtime, so an automatic list produces a package that installs
# cleanly and then cannot open a window.
#
# To regenerate after a dependency change:
#     strings -a target/.../schemaic | grep -oE 'lib[a-zA-Z0-9_+-]+\.so(\.[0-9]+)*' | sort -u
#
# These are Debian's *package* names, where the rpm spec requires *sonames*
# instead. That asymmetry is not a stylistic choice: rpm distributions
# auto-provide `libfoo.so.N()(64bit)` so one spec covers Fedora, RHEL and
# openSUSE, while dpkg has no soname provides and the package name is the only
# thing to depend on.
#
# libc6 (>= 2.31) restates the zigbuild floor rather than the binary's actual
# high-water mark (GLIBC_2.30 as of 0.16.3): the build target is the promise
# being made, and it is the number that stays true when the code changes.
DEPENDS="libc6 (>= 2.31)"
DEPENDS="${DEPENDS}, libxkbcommon0, libxkbcommon-x11-0"
DEPENDS="${DEPENDS}, libwayland-client0, libwayland-egl1"
DEPENDS="${DEPENDS}, libx11-6, libx11-xcb1, libxcb1"
DEPENDS="${DEPENDS}, libvulkan1, libegl1"

INSTALLED_SIZE="$(du -ks --exclude=DEBIAN "$ROOT" | cut -f1)"

cat > "${ROOT}/DEBIAN/control" <<EOF
Package: schemaic
Version: ${VERSION}
Architecture: ${DEB_ARCH}
Maintainer: ${MAINTAINER}
Installed-Size: ${INSTALLED_SIZE}
Depends: ${DEPENDS}
Recommends: mesa-vulkan-drivers
Section: database
Priority: optional
Homepage: https://github.com/fadion/schemaic
Description: Fast, native SQL editor for MySQL, MariaDB, PostgreSQL and SQLite
 Schemaic is a native SQL editor built to feel instant: a GPU-rendered
 interface that scrolls 200k-row result sets smoothly, an editable results
 grid, visual schema editing, and completion and diagnostics that follow the
 dialect you are connected to.
 .
 It connects to MySQL, MariaDB, PostgreSQL and SQLite, directly or over an SSH
 tunnel, and keeps credentials in the OS keyring. There is no account, no
 telemetry and no cloud service.
 .
 Schemaic is in active development and should not be trusted with production
 data.
EOF

mkdir -p "$OUTPUT_DIR"
OUT="${OUTPUT_DIR}/schemaic_${VERSION}_${DEB_ARCH}.deb"
# --root-owner-group so the package does not carry the build user's uid, which
# would otherwise be whatever the CI runner happens to use.
dpkg-deb --root-owner-group --build "$ROOT" "$OUT" >/dev/null
echo "$OUT"
