#!/usr/bin/env bash
#
# Install Schemaic on Linux or macOS.
#
#     curl -fsSL https://raw.githubusercontent.com/fadion/schemaic/main/install.sh | bash
#
# Picks the right route for this machine: the .pkg on macOS, the signed apt
# repository on Debian and Ubuntu, the signed dnf/zypper repository on Fedora,
# RHEL and openSUSE, and the self-updating AppImage on every other Linux.
#
# **Every one of those updates itself, by a different mechanism, and the script
# says which at the end.** The .pkg and the AppImage are Velopack installs and
# poll GitHub on their own. A .deb or .rpm lands in /usr/bin, which is not a
# Velopack install, so the in-app check correctly never runs - those come from
# the package repositories at https://fadion.github.io/schemaic and are carried
# forward by `apt-get upgrade` / `dnf upgrade` with the rest of the system.
#
# Set SCHEMAIC_NO_REPO=1 to install a single downloaded .deb or .rpm instead,
# adding nothing to the system's source lists. That build does not update
# itself and re-running this script is the only way forward from it, which is
# the trade being made.
#
# On macOS this script is also the way past Gatekeeper, and not by defeating
# it: the quarantine flag is set by whatever downloads a file, and curl does
# not set it. Nothing here disables a security check.
#
set -euo pipefail

REPO="fadion/schemaic"
API="https://api.github.com/repos/${REPO}/releases/latest"
RAW="https://raw.githubusercontent.com/${REPO}/main"
APP_ID="io.github.fadion.Schemaic"

# The package repositories. This URL is written into the user's source list and
# their machine will keep asking for it for as long as Schemaic is installed,
# so it is as permanent as the Velopack channel names - moving it means every
# existing install stops seeing updates, silently, with no route back to those
# users to tell them. Change it only alongside a plan for that.
SITE="https://fadion.github.io/schemaic"
KEYRING="/usr/share/keyrings/schemaic-archive-keyring.gpg"

# The repository signing key's fingerprint, as an **independent** channel.
#
# This script is served from raw.githubusercontent.com and the key from
# fadion.github.io — two origins — so a constant here is a check on the key that
# does not come from the same server the key does. That is the whole argument
# README.md makes for publishing the fingerprint at all: "a fingerprint you can
# only check against the same server the key came from is not a check at all,
# and this repository's history is a channel that server does not control." The
# landing page prints the fingerprint beside the key, which is not that channel.
#
# Without this the only check was a *shape* test — the first byte is 0x98/0x99/
# 0xc6, or the file says BEGIN PGP PUBLIC KEY BLOCK — which any attacker-made
# key passes. On Debian the substituted key then signs every future
# `apt-get upgrade` of schemaic, root-run, for the life of the install; on RPM
# `rpm --import` puts it in the **global** rpm keyring, where it validates
# packages from any repository on the machine, permanently, and no line in the
# uninstall instructions removes it.
#
# **Rotating the key means editing this constant and README.md's copy together**,
# and saying so in the release notes: an installed machine keeps the old key
# until someone re-runs this. `packaging/repo/README.md` carries that procedure.
KEY_FINGERPRINT="ABDBDC3958F3FAFC734273796566ECED7795DC1A"

if [ -t 1 ]; then
    RED=$'\033[0;31m'
    GREEN=$'\033[0;32m'
    YELLOW=$'\033[0;33m'
    BLUE=$'\033[0;34m'
    BOLD=$'\033[1m'
    RESET=$'\033[0m'
else
    RED=""
    GREEN=""
    YELLOW=""
    BLUE=""
    BOLD=""
    RESET=""
fi

info() { printf '%s[*]%s %s\n' "$BLUE" "$RESET" "$*"; }
ok() { printf '%s[+] %s%s\n' "$GREEN" "$*" "$RESET"; }
warn() { printf '%s[!] %s%s\n' "$YELLOW" "$*" "$RESET"; }
err() { printf '%s[x] %s%s\n' "$RED" "$*" "$RESET" >&2; }

has() { command -v "$1" >/dev/null 2>&1; }

# Read from the terminal rather than stdin. Without this, anything that prompts
# reads the *script itself* when it arrives through `curl | bash` and answers
# the question with a line of its own source.
tty_stdin() {
    if [ -r /dev/tty ]; then
        "$@" </dev/tty
    else
        "$@"
    fi
}

run_privileged() {
    if [ "$(id -u)" -eq 0 ]; then
        "$@"
    elif has sudo; then
        tty_stdin sudo "$@"
    else
        err "need root to install, and sudo is not available; re-run this script as root"
        exit 1
    fi
}

# The two platforms ship opposite architectures - Linux x86_64, macOS Apple
# Silicon - so the check has to know which one it is on. Without it a machine
# downloads the build for the other ISA and finds out at install time, or worse
# at launch.
os="$(uname -s)"
arch="$(uname -m)"
case "$os" in
    Linux) want_arch=x86_64 ;;
    Darwin) want_arch=arm64 ;;
    *)
        err "Schemaic has no build for ${os}. Windows users want the installer"
        err "from https://github.com/${REPO}/releases/latest"
        exit 1
        ;;
esac
if [ "$arch" != "$want_arch" ]; then
    err "Schemaic publishes ${want_arch} builds for ${os}, and this machine is ${arch}."
    err "Building from source is documented at https://github.com/${REPO}#build--run"
    exit 1
fi

fetch() {
    if has curl; then
        curl -fsSL --retry 3 --retry-delay 2 "$1"
    elif has wget; then
        wget -qO- --tries=3 "$1"
    else
        err "neither curl nor wget is installed"
        exit 1
    fi
}

download_to() {
    if has curl; then
        curl -fL --retry 3 --retry-delay 2 -o "$2" "$1"
    else
        wget --tries=3 -O "$2" "$1"
    fi
}

# The fingerprint of the primary key in `$1`, upper-case hex, or empty.
#
# **Two implementations because the binary keyring exists for machines with no
# `gpg`.** A slim container often has none — which is precisely why the
# dearmored keyring is published beside the armoured one — and "we could not
# check, so we installed it anyway" is not a check. So where `gpg` is missing
# the fingerprint is computed directly, which needs only `sha1sum`, `od` and
# `dd`.
#
# An OpenPGP v4 fingerprint is `SHA-1(0x99 || uint16(len) || packet body)` over
# the **primary public-key packet**, which is the first packet of an exported
# keyring. It depends on nothing that changes over the key's life — not the
# user IDs, not an extended expiry, not a new subkey — which is why this is the
# constant to pin rather than a hash of the exported file.
key_fingerprint() {
    local f="$1" out
    if has gpg; then
        out="$(gpg --batch --with-colons --show-keys --fingerprint "$f" 2>/dev/null \
            | awk -F: '/^fpr:/ { print $10; exit }')"
        if [ -n "$out" ]; then
            printf '%s\n' "$out"
            return 0
        fi
        # An armoured file `gpg` refused is a broken file, not a reason to fall
        # through to a parser that only understands the binary form.
        if grep -q 'BEGIN PGP PUBLIC KEY BLOCK' "$f" 2>/dev/null; then
            return 1
        fi
    fi
    has sha1sum && has od && has dd || return 1

    # **The RPM branch downloads the armoured key**, so without `gpg` the parser
    # below would refuse every RPM install rather than only the ones it cannot
    # check. Strip the armour first: the payload is base64 between the blank line
    # after the header and the `=CRC` line, and `base64` is coreutils like the
    # rest of this.
    if grep -q 'BEGIN PGP PUBLIC KEY BLOCK' "$f" 2>/dev/null; then
        has base64 || return 1
        local bin="${f}.dearmored"
        sed -n '/-----BEGIN PGP PUBLIC KEY BLOCK-----/,/-----END PGP PUBLIC KEY BLOCK-----/p' "$f" \
            | sed '1d' \
            | sed -n '/^$/,$p' \
            | sed '1d' \
            | sed '/^=/,$d' \
            | base64 -d > "$bin" 2>/dev/null || { rm -f "$bin"; return 1; }
        [ -s "$bin" ] || { rm -f "$bin"; return 1; }
        out="$(key_fingerprint "$bin")" || { rm -f "$bin"; return 1; }
        rm -f "$bin"
        printf '%s\n' "$out"
        return 0
    fi

    local b0 b1 len off
    b0="$(od -An -tx1 -N1 "$f" | tr -d ' \n')"
    case "$b0" in
        # Old format, tag 6 (public key), with a 1- or 2-byte length.
        98) len=$((0x$(od -An -tx1 -j1 -N1 "$f" | tr -d ' \n'))); off=2 ;;
        99) len=$((0x$(od -An -tx1 -j1 -N2 "$f" | tr -d ' \n'))); off=3 ;;
        # New format, tag 6, with a 1-, 2- or 5-byte length.
        c6)
            b1=$((0x$(od -An -tx1 -j1 -N1 "$f" | tr -d ' \n')))
            if [ "$b1" -lt 192 ]; then
                len=$b1; off=2
            elif [ "$b1" -lt 224 ]; then
                len=$(( ((b1 - 192) << 8) + $((0x$(od -An -tx1 -j2 -N1 "$f" | tr -d ' \n'))) + 192 ))
                off=3
            elif [ "$b1" -eq 255 ]; then
                len=$((0x$(od -An -tx1 -j2 -N4 "$f" | tr -d ' \n'))); off=6
            else
                return 1
            fi
            ;;
        *) return 1 ;;
    esac
    [ "$len" -gt 0 ] || return 1

    {
        printf '\231'
        printf "$(printf '\\%03o\\%03o' $((len / 256)) $((len % 256)))"
        dd if="$f" bs=1 skip="$off" count="$len" 2>/dev/null
    } | sha1sum | cut -d' ' -f1 | tr 'a-f' 'A-F'
}

# Refuse a signing key that is not the published one.
#
# Fails closed in every direction: a mismatch, an unreadable key, or a machine
# with neither `gpg` nor `sha1sum`. The last is the case that costs an install,
# and it is the right trade — the alternative is trusting a key nobody checked,
# on the one file whose whole job is to decide what this machine will run as
# root from now on. The message says exactly which tool would let it proceed.
require_published_key() {
    local f="$1" got
    if ! got="$(key_fingerprint "$f")" || [ -z "$got" ]; then
        err "could not read the downloaded signing key's fingerprint."
        err "Install 'gnupg' (or coreutils, for sha1sum) and re-run; the key is not"
        err "installed unchecked."
        return 1
    fi
    if [ "$got" != "$KEY_FINGERPRINT" ]; then
        err "the downloaded signing key is NOT the published Schemaic key."
        err "  expected ${KEY_FINGERPRINT}"
        err "  got      ${got}"
        err "Nothing has been installed. Either the key was rotated and this script is"
        err "out of date, or something between you and ${SITE} replaced it."
        return 1
    fi
    if ! only_one_key "$f"; then
        err "the downloaded signing key file holds more than one key."
        err "The published key is the only one that belongs in it. Nothing has been"
        err "installed. Either the key was rotated and this script is out of date, or"
        err "something between you and ${SITE} added a key to it."
        return 1
    fi
    ok "Signing key verified: ${KEY_FINGERPRINT}"
}

# Does `$1` hold exactly one public key?
#
# **A fingerprint is a claim about one key; the file is what gets installed.**
# An OpenPGP keyring is a packet stream, so `cat real.gpg attacker.gpg` is a
# valid keyring every tool unpacks — and `key_fingerprint` reads only the first
# key in it (the `exit` in the awk; the hand parser hashes packet 0 and never
# looks further). So the genuine key, with a second key concatenated after it,
# passed the check and printed "Signing key verified" — and then the **whole
# file** was installed: apt trusts every key in a `Signed-By` keyring, and
# `rpm --import` imports every block in the file into the machine's *global*
# keyring, where it validates packages from any repository, permanently, and no
# line of the uninstall instructions removes it.
#
# Counting `^pub:` records is the narrow version of "install only the key you
# verified": it refuses the composed file rather than re-exporting from it, so
# the no-`gpg` path keeps working the way the rest of this script does — by
# refusing what it cannot check rather than proceeding unchecked.
only_one_key() {
    local f="$1" n bin rc
    if has gpg; then
        n="$(gpg --batch --with-colons --show-keys "$f" 2>/dev/null | grep -c '^pub:' || true)"
        [ "$n" = 1 ]
        return $?
    fi
    # Without gpg, the same question is "is there anything after the first
    # packet's declared length" — for the armoured form, after dearmouring it.
    if grep -q 'BEGIN PGP PUBLIC KEY BLOCK' "$f" 2>/dev/null; then
        has base64 || return 1
        # More than one armour block in one file is the same attack, spelled
        # differently, and needs no parsing to see.
        [ "$(grep -c 'BEGIN PGP PUBLIC KEY BLOCK' "$f")" = 1 ] || return 1
        bin="${f}.onekey"
        sed -n '/-----BEGIN PGP PUBLIC KEY BLOCK-----/,/-----END PGP PUBLIC KEY BLOCK-----/p' "$f" \
            | sed '1d' \
            | sed -n '/^$/,$p' \
            | sed '1d' \
            | sed '/^=/,$d' \
            | base64 -d > "$bin" 2>/dev/null || { rm -f "$bin"; return 1; }
        only_one_key "$bin"
        rc=$?
        rm -f "$bin"
        return $rc
    fi
    # A binary keyring, with no gpg: walk the packet stream. Subkeys, user IDs
    # and signatures are tags 14, 13 and 2 and are all part of the one key; a
    # second **tag 6** is a second key. The deb route publishes the dearmoured
    # keyring precisely so a slim container needs no gpg, so this path has to
    # answer rather than refuse — and it walks the same header shapes
    # `key_fingerprint` already decodes one of.
    n="$(count_public_key_packets "$f")" || return 1
    [ "$n" = 1 ]
}

# How many OpenPGP public-key packets (tag 6) are in the binary file `$1`, or a
# non-zero exit if the stream does not walk cleanly to its end.
#
# A malformed or truncated stream is a failure, not a count: the whole point is
# that the bytes about to be installed are the bytes that were verified, and a
# file this cannot account for is not that.
count_public_key_packets() {
    local f="$1" size off b0 tag ltype b1 len hdr n
    has od && has wc || return 1
    size="$(wc -c < "$f" | tr -d ' ')"
    off=0
    n=0
    while [ "$off" -lt "$size" ]; do
        b0=$((0x$(od -An -tx1 -j"$off" -N1 "$f" | tr -d ' \n')))
        # Bit 7 is set on every packet header; anything else is not a stream.
        [ $((b0 & 128)) -ne 0 ] || return 1
        if [ $((b0 & 64)) -eq 0 ]; then
            # Old format: tag in bits 5-2, length type in bits 1-0.
            tag=$(((b0 >> 2) & 15))
            ltype=$((b0 & 3))
            case "$ltype" in
                0) len=$((0x$(od -An -tx1 -j$((off + 1)) -N1 "$f" | tr -d ' \n'))); hdr=2 ;;
                1) len=$((0x$(od -An -tx1 -j$((off + 1)) -N2 "$f" | tr -d ' \n'))); hdr=3 ;;
                2) len=$((0x$(od -An -tx1 -j$((off + 1)) -N4 "$f" | tr -d ' \n'))); hdr=5 ;;
                # Indeterminate length: runs to EOF, so nothing can follow it and
                # nothing here can check that. Refuse.
                *) return 1 ;;
            esac
        else
            # New format: tag in bits 5-0, then a 1-, 2- or 5-byte length.
            tag=$((b0 & 63))
            b1=$((0x$(od -An -tx1 -j$((off + 1)) -N1 "$f" | tr -d ' \n')))
            if [ "$b1" -lt 192 ]; then
                len=$b1
                hdr=2
            elif [ "$b1" -lt 224 ]; then
                len=$((((b1 - 192) << 8) + $((0x$(od -An -tx1 -j$((off + 2)) -N1 "$f" | tr -d ' \n'))) + 192))
                hdr=3
            elif [ "$b1" -eq 255 ]; then
                len=$((0x$(od -An -tx1 -j$((off + 2)) -N4 "$f" | tr -d ' \n')))
                hdr=6
            else
                # Partial body length — not used by an exported key.
                return 1
            fi
        fi
        if [ "$tag" -eq 6 ]; then
            n=$((n + 1))
        fi
        off=$((off + hdr + len))
        # Past the end means the declared length lied about the file.
        [ "$off" -le "$size" ] || return 1
    done
    printf '%s\n' "$n"
}

# Refuse a downloaded repository configuration that does not name this site.
#
# **The fingerprint check guards the key; nothing guarded the file that decides
# what the key is used for.** Both configs were fetched from the same origin as
# the key and accepted on one shape grep each — so an adversary serving the
# *genuine* key alongside a config naming their own `URIs:`/`baseurl=` got
# "Repository added, signed by the published key" printed at them, and every
# `apt-get upgrade`/`dnf upgrade` on that machine fetched root-installed
# packages from their host for the life of the install. deb822 also accepts
# `Trusted: yes`, which turns apt's signature verification off for the entry
# entirely, and an *inline* armoured key in `Signed-By:` — so the attacker's key
# can travel in the file that was never checked rather than the one that was.
#
# `SITE` and `KEYRING` are constants this script already holds; it just never
# made the comparison. A rotation of `SITE` has to touch this file anyway (the
# `pages.yml` guard enforces it), so these cannot drift silently.
require_expected_line() {
    local f="$1" want="$2"
    grep -qx -- "$want" "$f" && return 0
    err "the downloaded repository configuration does not match this installer."
    err "  expected a line: ${want}"
    err "Nothing has been installed. Either the repository moved and this script is"
    err "out of date, or something between you and ${SITE} replaced it."
    return 1
}

# Match a release asset by file-name pattern. Anonymous GitHub API calls are
# limited to 60/hour per address, and this is the script's only one.
asset_url() {
    local url
    url="$(fetch "$API" | grep -Eo "https://[^\"]+$1" | head -n1 || true)"
    if [ -z "$url" ]; then
        err "no asset matching '$1' in the latest release of ${REPO}"
        exit 1
    fi
    printf '%s\n' "$url"
}

detect_family() {
    # **`dnf` and `zypper` are diagnostic; the bare `rpm` binary is not.** A
    # machine with dnf and apt is an rpm machine that has picked up apt somehow,
    # not the reverse — but `rpm` on its own is an ordinary Debian/Ubuntu
    # package, and this repository's own CI runners install it on
    # `ubuntu-latest` (release.yml, pages.yml). Answering `rpm` for those steered
    # them off the *signed apt repository* and onto `rpm -i --nosignature`, the
    # one branch that installs an unsigned package with the check explicitly
    # waived — a trust posture chosen by a predicate that was wrong about the
    # machine. It did not even complete: the spec's nine soname `Requires:`
    # cannot be met by an empty rpm database, so `rpm -i` failed, `set -e` ended
    # the script, and an Ubuntu user got an rpm error with no hint that
    # `SCHEMAIC_PKG_FAMILY=debian` existed.
    if has dnf || has zypper; then
        echo rpm
    elif has apt-get || has dpkg; then
        echo debian
    # An rpm machine with neither front-end — and only once dpkg has been ruled
    # out, which is the whole correction.
    elif has rpm; then
        echo rpm
    else
        echo unknown
    fi
}

# Adding a third-party repository is a bigger thing to do to someone's machine
# than dropping a package on it, and it is the whole point: apt will keep
# fetching from here, so this is also the only route on which an upgrade
# arrives without the user coming back.
install_deb() {
    if [ "${SCHEMAIC_NO_REPO:-0}" = 1 ]; then
        install_deb_direct
        return
    fi

    local tmp first
    tmp="$(mktemp -d)"
    info "Adding the Schemaic apt repository (${SITE}/deb)"

    # The dearmored keyring is published beside the armoured key precisely so
    # this needs no gpg on the machine - a slim container often has none, and
    # `gpg --dearmor` would be an extra dependency for a file we can just as
    # easily publish in both forms.
    download_to "${SITE}/schemaic-archive-keyring.gpg" "${tmp}/keyring.gpg"
    # A 404 or a captive-portal page arrives here looking like a file, and apt
    # would then reject every update with an unhelpful signature error. A
    # keyring begins with an OpenPGP public-key packet: 0x98, 0x99 or 0xc6.
    first="$(od -An -tx1 -N1 "${tmp}/keyring.gpg" | tr -d ' \n')"
    case "$first" in
        98 | 99 | c6) ;;
        *)
            err "the downloaded signing key is not a GPG keyring (got bytes '${first}')"
            rm -rf "$tmp"
            exit 1
            ;;
    esac
    # …and a shape test is not an identity test. Any attacker-made key passes the
    # one above; this one is the check — see `KEY_FINGERPRINT`.
    if ! require_published_key "${tmp}/keyring.gpg"; then
        rm -rf "$tmp"
        exit 1
    fi

    download_to "${SITE}/schemaic.sources" "${tmp}/schemaic.sources"
    if ! grep -q '^Types: deb' "${tmp}/schemaic.sources"; then
        err "the downloaded apt source is not a deb822 sources file"
        rm -rf "$tmp"
        exit 1
    fi
    # …and a shape test is not an identity test here either. The two lines that
    # decide what this machine will install as root from now on are `URIs:` and
    # `Signed-By:`; `Trusted: yes` turns apt's signature checking off for the
    # entry entirely, and an inline armoured key in `Signed-By:` would carry a
    # key past the fingerprint check that only ever looked at the other file.
    if ! require_expected_line "${tmp}/schemaic.sources" "URIs: ${SITE}/deb" \
        || ! require_expected_line "${tmp}/schemaic.sources" "Signed-By: ${KEYRING}"; then
        rm -rf "$tmp"
        exit 1
    fi
    if grep -qi '^Trusted:' "${tmp}/schemaic.sources"; then
        err "the downloaded apt source asks apt to trust it without a signature."
        err "Nothing has been installed."
        rm -rf "$tmp"
        exit 1
    fi

    info "Installing the repository (this needs root)"
    run_privileged install -m 0644 -D "${tmp}/keyring.gpg" "$KEYRING"
    run_privileged install -m 0644 -D "${tmp}/schemaic.sources" /etc/apt/sources.list.d/schemaic.sources
    rm -rf "$tmp"
    ok "Repository added, signed by the published key"

    # Not fatal. A machine with somebody else's broken PPA in its lists fails
    # `apt-get update` as a whole, and that is not a reason to refuse to
    # install Schemaic - if our own source is the broken one, the install below
    # says so precisely.
    if ! run_privileged apt-get update; then
        warn "apt-get update reported an error, often from an unrelated repository; continuing"
    fi
    info "Installing schemaic"
    run_privileged apt-get install -y schemaic
    ok "Installed"
}

# The pre-repository route, kept for SCHEMAIC_NO_REPO=1: one package, nothing
# added to the system's source lists, and no updates.
install_deb_direct() {
    local url tmp
    url="$(asset_url '_amd64\.deb')"
    tmp="$(mktemp --suffix=.deb)"
    info "Downloading ${url##*/}"
    download_to "$url" "$tmp"
    # A truncated download or an HTML error page arrives here looking like a
    # file; dpkg-deb is the cheapest way to learn that it is not one.
    if ! dpkg-deb -I "$tmp" >/dev/null 2>&1; then
        rm -f "$tmp"
        err "the downloaded file is not a valid .deb"
        exit 1
    fi
    ok "Downloaded ${url##*/}"

    info "Installing with apt-get (this needs root)"
    # `apt-get install ./file.deb` resolves the package's dependencies from the
    # configured repositories; `dpkg -i` would leave them unmet.
    run_privileged apt-get install -y "$tmp"
    rm -f "$tmp"
    ok "Installed"
}

install_rpm() {
    if [ "${SCHEMAIC_NO_REPO:-0}" = 1 ] || ! { has dnf || has zypper; }; then
        # Plain rpm with no dnf and no zypper has no repository support worth
        # the name, so that machine takes the direct route whatever it asked
        # for - and is told so.
        if [ "${SCHEMAIC_NO_REPO:-0}" != 1 ]; then
            warn "neither dnf nor zypper is available; installing a single package instead of adding the repository"
        fi
        install_rpm_direct
        return
    fi

    local tmp
    tmp="$(mktemp -d)"
    info "Adding the Schemaic package repository (${SITE}/rpm)"

    download_to "${SITE}/schemaic.asc" "${tmp}/schemaic.asc"
    if ! grep -q 'BEGIN PGP PUBLIC KEY BLOCK' "${tmp}/schemaic.asc"; then
        err "the downloaded signing key is not an armoured GPG key"
        rm -rf "$tmp"
        exit 1
    fi
    # Checked *before* `rpm --import`, which is the larger of the two
    # escalations: the rpm database's keyring is not scoped to this repository,
    # so a substituted key would validate packages from any repository on this
    # machine, permanently, and no line of the uninstall instructions removes it.
    if ! require_published_key "${tmp}/schemaic.asc"; then
        rm -rf "$tmp"
        exit 1
    fi
    download_to "${SITE}/schemaic.repo" "${tmp}/schemaic.repo"
    if ! grep -q '^\[schemaic\]' "${tmp}/schemaic.repo"; then
        err "the downloaded repository definition is not a .repo file"
        rm -rf "$tmp"
        exit 1
    fi
    # The four lines that decide where root-installed packages come from and
    # whether anything checks them. `dnf install -y` below imports whatever
    # `gpgkey=` names with no prompt, so an unchecked `.repo` defeats the key
    # check entirely rather than merely weakening it.
    if ! require_expected_line "${tmp}/schemaic.repo" "baseurl=${SITE}/rpm" \
        || ! require_expected_line "${tmp}/schemaic.repo" "gpgkey=${SITE}/schemaic.asc" \
        || ! require_expected_line "${tmp}/schemaic.repo" "gpgcheck=1" \
        || ! require_expected_line "${tmp}/schemaic.repo" "repo_gpgcheck=1"; then
        rm -rf "$tmp"
        exit 1
    fi

    # Imported into the rpm database up front so the packages verify against a
    # key the machine already holds. Without this, dnf offers to import it
    # mid-install, which is a prompt in the middle of a piped script and a much
    # worse moment to be deciding whether to trust a key.
    info "Importing the signing key (this needs root)"
    run_privileged rpm --import "${tmp}/schemaic.asc"

    if has dnf; then
        run_privileged install -m 0644 -D "${tmp}/schemaic.repo" /etc/yum.repos.d/schemaic.repo
        rm -rf "$tmp"
        ok "Repository added, with signature checking on"
        info "Installing schemaic"
        run_privileged dnf install -y schemaic
    else
        # zypper reads its own directory, not /etc/yum.repos.d.
        run_privileged install -m 0644 -D "${tmp}/schemaic.repo" /etc/zypp/repos.d/schemaic.repo
        rm -rf "$tmp"
        ok "Repository added, with signature checking on"
        info "Installing schemaic"
        run_privileged zypper --non-interactive refresh schemaic
        run_privileged zypper --non-interactive install schemaic
    fi
    ok "Installed"
}

# The pre-repository route, kept for SCHEMAIC_NO_REPO=1 and for machines with
# no dnf or zypper: one package, nothing added to the system, no updates. The
# .rpm on the Releases page is unsigned - only the copies in the repository are
# signed - which is why every branch below waives the signature check.
install_rpm_direct() {
    local url tmp
    url="$(asset_url '\.x86_64\.rpm')"
    tmp="$(mktemp --suffix=.rpm)"
    info "Downloading ${url##*/}"
    download_to "$url" "$tmp"
    if ! rpm -qp "$tmp" >/dev/null 2>&1; then
        rm -f "$tmp"
        err "the downloaded file is not a valid .rpm"
        exit 1
    fi
    ok "Downloaded ${url##*/}"

    # Worth stating rather than burying: waiving the check means this download
    # is trusted because of where it came from, and nothing else. The copies in
    # the repository are signed, which is the reason to prefer that route.
    warn "The .rpm on the Releases page is not GPG-signed; the install below waives the signature check."
    info "Installing (this needs root)"
    if has dnf; then
        run_privileged dnf install -y --nogpgcheck "$tmp"
    elif has zypper; then
        run_privileged zypper --non-interactive install --allow-unsigned-rpm "$tmp"
    else
        warn "only plain rpm is available, so dependencies will not be resolved for you"
        run_privileged rpm -i --nosignature "$tmp"
    fi
    rm -f "$tmp"
    ok "Installed"
}

# Everything that is neither Debian- nor RPM-based: Arch, NixOS, Alpine, Solus,
# Void. The AppImage needs no package manager and is also the only artifact
# that updates itself, so this is a fair default rather than a consolation
# prize.
install_appimage() {
    local url dest bindir desktop_dir icon_dir tmp_desktop
    bindir="${HOME}/.local/bin"
    desktop_dir="${HOME}/.local/share/applications"
    icon_dir="${HOME}/.local/share/icons/hicolor/512x512/apps"
    dest="${bindir}/Schemaic.AppImage"

    url="$(asset_url '\.AppImage')"
    mkdir -p "$bindir" "$desktop_dir" "$icon_dir"
    info "Downloading ${url##*/}"
    download_to "$url" "$dest"
    chmod +x "$dest"
    ok "Installed to ${dest}"

    # The AppImage carries a .desktop of its own inside it, but nothing reads
    # that until the file is registered with the desktop; without these two the
    # app exists only as a path to type.
    download_to "${RAW}/assets/icon.png" "${icon_dir}/${APP_ID}.png" || true
    tmp_desktop="$(mktemp)"
    fetch "${RAW}/packaging/linux/${APP_ID}.desktop" > "$tmp_desktop"
    # Drop whatever `Exec=` the upstream file carries and append ours, rather
    # than substituting a restated copy of it. The old spelling was
    # `sed "s|^Exec=schemaic$|Exec=${dest}|"`, a second copy of
    # `packaging/linux/<APP_ID>.desktop`'s own line, and `sed` substitutes zero
    # occurrences without an error: the ordinary next edit upstream
    # (`Exec=schemaic %U`, which is what makes "open with" work) would have left
    # the installed entry pointing at a `schemaic` that is not on PATH in an
    # AppImage install, under a printed "Added a desktop entry". The file is
    # fetched from `main` while the AppImage beside it comes from the latest
    # release, so the two can differ by any number of commits and nothing pins
    # them; an append cannot miss whatever that line says.
    #
    # `|| true` because `grep -v` exits 1 when it selects nothing and `set -e`
    # is on; the assertion below is what actually catches a bad fetch.
    if ! grep -q '^\[Desktop Entry\]' "$tmp_desktop"; then
        rm -f "$tmp_desktop"
        err "the downloaded desktop entry is not a desktop file"
        exit 1
    fi
    {
        grep -v '^Exec=' "$tmp_desktop" || true
        printf 'Exec=%s\n' "$dest"
    } > "${desktop_dir}/${APP_ID}.desktop"
    rm -f "$tmp_desktop"
    if has update-desktop-database; then
        update-desktop-database "$desktop_dir" >/dev/null 2>&1 || true
    fi
    ok "Added a desktop entry"

    case ":${PATH}:" in
        *":${bindir}:"*) ;;
        *) warn "${bindir} is not on your PATH; add it to launch Schemaic from a terminal" ;;
    esac
}

# macOS has exactly one route, so it never consults SCHEMAIC_PKG_FAMILY or the
# package-manager sniffing below - a Mac with Homebrew's `rpm` on it is still a
# Mac.
install_macos() {
    local url tmp
    url="$(asset_url '\.pkg')"
    tmp="$(mktemp -d)/Schemaic.pkg"
    info "Downloading ${url##*/}"
    download_to "$url" "$tmp"
    if ! pkgutil --check-signature "$tmp" >/dev/null 2>&1; then
        # Expected: the package is unsigned. Worth saying out loud rather than
        # discovering later, because it is the same trust posture as the
        # Windows installer - you are trusting where this came from, and
        # nothing else is vouching for it.
        warn "This package is not signed by an Apple Developer ID."
    fi
    info "Installing to /Applications (this needs root)"
    run_privileged installer -pkg "$tmp" -target /
    rm -f "$tmp"
    ok "Installed"
    # Only true for this path, and it is the reason this script is the pleasant
    # way in on macOS: the quarantine flag is set by whatever downloads a file,
    # and curl does not set it. A browser download of the same .pkg would need
    # a trip through System Settings before it would open.
    info "Downloaded with curl, so Gatekeeper's quarantine flag was never set."
}

if [ "$os" = Darwin ]; then
    family=macos
else
    family="${SCHEMAIC_PKG_FAMILY:-$(detect_family)}"
fi
case "$family" in
    debian | rpm | appimage | macos) ;;
    unknown)
        warn "No supported package manager found; the AppImage is the way in on this system."
        family=appimage
        ;;
    *)
        err "Invalid SCHEMAIC_PKG_FAMILY '${family}' (expected: debian, rpm, appimage)"
        exit 1
        ;;
esac
info "Installing the ${BOLD}${family}${RESET} build (set SCHEMAIC_PKG_FAMILY to override)"

case "$family" in
    debian) install_deb ;;
    rpm) install_rpm ;;
    appimage) install_appimage ;;
    macos) install_macos ;;
esac

echo
ok "Schemaic is installed."
case "$family" in
    debian)
        if [ "${SCHEMAIC_NO_REPO:-0}" = 1 ]; then
            info "Updates:   none - this is a single package, with no repository behind it."
            info "           Re-run this script without SCHEMAIC_NO_REPO to get them."
            info "Uninstall: sudo apt-get remove schemaic"
        else
            info "Updates:   with the rest of your system - sudo apt-get update && sudo apt-get upgrade."
            info "           To have unattended-upgrades pick it up too, add \"Schemaic:stable\";"
            info "           to Unattended-Upgrade::Allowed-Origins."
            info "Uninstall: sudo apt-get remove schemaic \\"
            info "           && sudo rm /etc/apt/sources.list.d/schemaic.sources ${KEYRING}"
        fi
        ;;
    rpm)
        if [ "${SCHEMAIC_NO_REPO:-0}" = 1 ] || { ! has dnf && ! has zypper; }; then
            info "Updates:   none - this is a single package, with no repository behind it."
            info "           Re-run this script without SCHEMAIC_NO_REPO to get them."
            # The removal command comes from the same test `install_rpm_direct`
            # branched on, not from the family: the second disjunct above is
            # reached *precisely when there is no dnf*, and printing
            # `sudo dnf remove` there names a command that is not on the
            # machine. The install went through `rpm -i --nosignature`, so the
            # way back out is `rpm -e`.
            if has dnf; then
                info "Uninstall: sudo dnf remove schemaic"
            elif has zypper; then
                info "Uninstall: sudo zypper remove schemaic"
            else
                info "Uninstall: sudo rpm -e schemaic"
            fi
        elif has dnf; then
            info "Updates:   with the rest of your system - sudo dnf upgrade."
            info "Uninstall: sudo dnf remove schemaic && sudo rm /etc/yum.repos.d/schemaic.repo"
        else
            info "Updates:   with the rest of your system - sudo zypper update."
            info "Uninstall: sudo zypper remove schemaic \\"
            info "           && sudo rm /etc/zypp/repos.d/schemaic.repo"
        fi
        ;;
    appimage)
        info "Updates:   checked automatically; the app offers a restart when one is staged."
        # All three files `install_appimage` writes, including the icon it
        # drops into the hicolor theme - following a line that omits one leaves
        # it behind.
        info "Uninstall: rm ~/.local/bin/Schemaic.AppImage \\"
        info "              ~/.local/share/applications/${APP_ID}.desktop \\"
        info "              ~/.local/share/icons/hicolor/512x512/apps/${APP_ID}.png"
        ;;
    macos)
        info "Updates:   checked automatically; the app offers a restart when one is staged."
        info "Uninstall: rm -rf /Applications/Schemaic.app"
        ;;
esac
printf '%sSchemaic is in active development - do not trust it with data you care about.%s\n' "$YELLOW" "$RESET"
