#!/usr/bin/env bash
#
# Build the whole GitHub Pages site: both package repositories, the client
# configuration files, the public key and the landing page.
#
#     packaging/repo/build-site.sh <output-dir>
#
# Environment:
#   GPG_KEY_ID            required - fingerprint or uid of the signing key
#   GPG_PASSPHRASE_FILE   optional - file holding that key passphrase
#   SCHEMAIC_REPO_URL     base URL the site will be served from
#   SCHEMAIC_RETAIN       how many releases to keep (default 5)
#   SCHEMAIC_GH_REPO      owner/name to pull release assets from
#
# **The GitHub Release assets are the source of truth and this output is
# derived.** Nothing here is incremental: every run downloads the packages
# again and rebuilds the whole site from scratch. That is the point - there is
# no accumulated state to corrupt, no branch whose history grows by 40 MB a
# release, and a repository that has gone wrong is repaired by re-running this
# rather than by unpicking what it did last time.
#
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <output-dir>" >&2
    exit 2
fi

OUT="$1"
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${HERE}/../.." && pwd)"

: "${GPG_KEY_ID:?set GPG_KEY_ID to the signing key fingerprint or uid}"
BASE_URL="${SCHEMAIC_REPO_URL:-https://fadion.github.io/schemaic}"
RETAIN="${SCHEMAIC_RETAIN:-5}"
GH_REPO="${SCHEMAIC_GH_REPO:-fadion/schemaic}"

command -v gh >/dev/null || { echo "gh CLI not found" >&2; exit 1; }

STAGE="$(mktemp -d)"
trap 'rm -rf "$STAGE"' EXIT
mkdir -p "${STAGE}/deb" "${STAGE}/rpm"

# Releases newest-first, drafts and pre-releases excluded. The limit is larger
# than RETAIN on purpose: .deb and .rpm only started shipping at v0.16.3, so
# walking back far enough to *find* RETAIN releases that have them is not the
# same as taking the first RETAIN tags.
echo "[site] collecting packages from the last ${RETAIN} release(s) of ${GH_REPO}"
mapfile -t tags < <(
    gh release list --repo "$GH_REPO" --limit 40 \
        --exclude-drafts --exclude-pre-releases \
        --json tagName --jq '.[].tagName'
)

kept=0
latest_version=""
for tag in "${tags[@]}"; do
    [ "$kept" -lt "$RETAIN" ] || break
    got=0
    tmp="${STAGE}/dl"
    rm -rf "$tmp"; mkdir -p "$tmp"
    if gh release download "$tag" --repo "$GH_REPO" --dir "$tmp" \
        --pattern '*.deb' --pattern '*.rpm' --clobber 2>/dev/null; then
        shopt -s nullglob
        for f in "$tmp"/*.deb; do cp "$f" "${STAGE}/deb/"; got=1; done
        for f in "$tmp"/*.rpm; do cp "$f" "${STAGE}/rpm/"; got=1; done
        shopt -u nullglob
    fi
    if [ "$got" -eq 1 ]; then
        kept=$((kept + 1))
        [ -n "$latest_version" ] || latest_version="${tag#v}"
        echo "[site]   ${tag}"
    else
        echo "[site]   ${tag} - no distribution packages, skipped"
    fi
done

if [ "$kept" -eq 0 ]; then
    echo "no release carried a .deb or .rpm; refusing to publish an empty repository" >&2
    exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT"

bash "${HERE}/build-apt-repo.sh" "${STAGE}/deb" "${OUT}/deb"
bash "${HERE}/build-rpm-repo.sh" "${STAGE}/rpm" "${OUT}/rpm"

# Two encodings of one key. APT's signed-by wants a binary keyring on every
# release we support (inline armoured keys need apt 2.4, which Debian 11 and
# Ubuntu 20.04 do not have); dnf's gpgkey= wants the armoured form and fetches
# it over HTTPS itself.
gpg --batch --yes --armor --export "$GPG_KEY_ID" > "${OUT}/schemaic.asc"
gpg --batch --yes --export "$GPG_KEY_ID" > "${OUT}/schemaic-archive-keyring.gpg"
[ -s "${OUT}/schemaic.asc" ] || { echo "exported an empty public key" >&2; exit 1; }

fingerprint="$(
    gpg --batch --with-colons --fingerprint "$GPG_KEY_ID" \
        | awk -F: '/^fpr:/ { print $10; exit }'
)"

# deb822. The older one-line form is published beside it because plenty of
# tooling and muscle memory still expects a .list, and both name the same
# keyring path so the two cannot disagree about trust.
cat > "${OUT}/schemaic.sources" <<EOF
Types: deb
URIs: ${BASE_URL}/deb
Suites: stable
Components: main
Architectures: amd64
Signed-By: /usr/share/keyrings/schemaic-archive-keyring.gpg
EOF

cat > "${OUT}/schemaic.list" <<EOF
deb [arch=amd64 signed-by=/usr/share/keyrings/schemaic-archive-keyring.gpg] ${BASE_URL}/deb stable main
EOF

# Both checks on: gpgcheck is the signature inside each package, repo_gpgcheck
# the one over the index. Leaving either off would make a repository that
# verifies less than the file it replaced.
cat > "${OUT}/schemaic.repo" <<EOF
[schemaic]
name=Schemaic
baseurl=${BASE_URL}/rpm
enabled=1
type=rpm-md
gpgcheck=1
repo_gpgcheck=1
gpgkey=${BASE_URL}/schemaic.asc
EOF

cp "${REPO_ROOT}/assets/icon.png" "${OUT}/icon.png"
sed -e "s|__BASE_URL__|${BASE_URL}|g" \
    -e "s|__VERSION__|${latest_version}|g" \
    -e "s|__FINGERPRINT__|${fingerprint}|g" \
    "${HERE}/index.html.in" > "${OUT}/index.html"

# Pages runs Jekyll over a branch source and would drop directories beginning
# with an underscore; the Actions deployment does not, but the file costs
# nothing and removes the question.
touch "${OUT}/.nojekyll"

echo
echo "[site] ${kept} release(s), latest ${latest_version}"
echo "[site] key ${fingerprint}"
echo "[site] built at ${OUT} for ${BASE_URL}"
du -sh "$OUT"
