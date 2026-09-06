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
    # dnf/zypper before apt: a machine with both is an rpm machine that has
    # picked up apt somehow, not the reverse.
    if has dnf || has zypper || has rpm; then
        echo rpm
    elif has apt-get || has dpkg; then
        echo debian
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

    download_to "${SITE}/schemaic.sources" "${tmp}/schemaic.sources"
    if ! grep -q '^Types: deb' "${tmp}/schemaic.sources"; then
        err "the downloaded apt source is not a deb822 sources file"
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
    download_to "${SITE}/schemaic.repo" "${tmp}/schemaic.repo"
    if ! grep -q '^\[schemaic\]' "${tmp}/schemaic.repo"; then
        err "the downloaded repository definition is not a .repo file"
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
    sed "s|^Exec=schemaic\$|Exec=${dest}|" "$tmp_desktop" > "${desktop_dir}/${APP_ID}.desktop"
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
            info "Uninstall: sudo dnf remove schemaic"
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
        info "Uninstall: rm ~/.local/bin/Schemaic.AppImage \\"
        info "              ~/.local/share/applications/${APP_ID}.desktop"
        ;;
    macos)
        info "Updates:   checked automatically; the app offers a restart when one is staged."
        info "Uninstall: rm -rf /Applications/Schemaic.app"
        ;;
esac
printf '%sSchemaic is in active development - do not trust it with data you care about.%s\n' "$YELLOW" "$RESET"
