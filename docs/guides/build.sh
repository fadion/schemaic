#!/usr/bin/env bash
# Build the user guide into <output-dir> with a pinned, checksummed mdBook.
#
#     docs/guides/build.sh <output-dir>
#
# mdBook empties <output-dir> before it writes, so a directory that is not
# empty and holds no earlier build of this book (no `searchindex.js`) is
# refused: `build.sh .` or `build.sh ~/notes` would delete what is there.
#
# Both ci.yml (on every push, so a broken guide is caught there) and pages.yml
# (on a release, into the site's guide/ folder) call this, so the version and
# its checksum live here once. Linux x86_64 only — locally, `cargo install
# mdbook --version <MDBOOK_VERSION>` and `mdbook build docs/guides` do the same.
set -euo pipefail

MDBOOK_VERSION="0.4.52"
MDBOOK_SHA256="c0b903f01dd8f4edc644372ad2b80b1fdddd12552d37b6a098657cbd8eddd768"

OUT="${1:?usage: build.sh <output-dir> (emptied first; must be new, empty or an earlier build)}"
if [ -d "$OUT" ] && [ -n "$(ls -A "$OUT")" ] && [ ! -f "$OUT/searchindex.js" ]; then
    echo "build.sh: $OUT is not empty and holds no earlier build of the guide;" \
        "mdBook would delete what is in it — give a new or empty directory" >&2
    exit 2
fi
HERE="$(cd "$(dirname "$0")" && pwd)"
TOOLS="$(mktemp -d)"
trap 'rm -rf "$TOOLS"' EXIT

# Bounded: `--retry` does not retry a transfer that hangs rather than fails.
curl -fsSL --retry 3 --connect-timeout 20 --max-time 120 -o "${TOOLS}/mdbook.tar.gz" \
    "https://github.com/rust-lang/mdBook/releases/download/v${MDBOOK_VERSION}/mdbook-v${MDBOOK_VERSION}-x86_64-unknown-linux-gnu.tar.gz"
echo "${MDBOOK_SHA256}  ${TOOLS}/mdbook.tar.gz" | sha256sum --check --quiet
tar -xzf "${TOOLS}/mdbook.tar.gz" -C "$TOOLS"

"${TOOLS}/mdbook" build "$HERE" --dest-dir "$(realpath -m "$OUT")"
