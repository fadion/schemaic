#!/usr/bin/env bash
#
# Install Schemaic on Linux.
#
#     curl -fsSL https://raw.githubusercontent.com/fadion/schemaic/main/install.sh | bash
#
# Picks the right artifact from the latest GitHub Release for this machine: a
# .deb on Debian and Ubuntu, an .rpm on Fedora, RHEL and openSUSE, and the
# self-updating AppImage everywhere else.
#
# **The three are not equivalent, and the script says so at the end.** The
# AppImage is a Velopack install and checks for updates on its own. A .deb or
# .rpm lands in /usr/bin, which is not a Velopack install, so the in-app check
# correctly never runs - those are updated by re-running this script, until
# there is an apt/dnf repository to point at.
#
set -euo pipefail

REPO="fadion/schemaic"
API="https://api.github.com/repos/${REPO}/releases/latest"
RAW="https://raw.githubusercontent.com/${REPO}/main"
APP_ID="io.github.fadion.Schemaic"

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

# Only x86_64 is published. Checking is the whole point: without it an arm64
# machine downloads an amd64 package and finds out at install time - or worse,
# at launch.
arch="$(uname -m)"
if [ "$arch" != "x86_64" ]; then
    err "Schemaic publishes x86_64 Linux builds only, and this machine is ${arch}."
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

install_deb() {
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

    # Unsigned on purpose (see the packaging notes in the release workflow), so
    # every branch below waives its signature check. Worth stating rather than
    # burying: it means the download is trusted because of where it came from,
    # and nothing else.
    warn "Schemaic's packages are not GPG-signed; the install below waives the signature check."
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

family="${SCHEMAIC_PKG_FAMILY:-$(detect_family)}"
case "$family" in
    debian | rpm | appimage) ;;
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
esac

echo
ok "Schemaic is installed."
case "$family" in
    debian)
        info "Updates:   this build does not update itself - re-run this script for the next"
        info "           release, or use SCHEMAIC_PKG_FAMILY=appimage, which does."
        info "Uninstall: sudo apt-get remove schemaic"
        ;;
    rpm)
        info "Updates:   this build does not update itself - re-run this script for the next"
        info "           release, or use SCHEMAIC_PKG_FAMILY=appimage, which does."
        info "Uninstall: sudo dnf remove schemaic"
        ;;
    appimage)
        info "Updates:   checked automatically; the app offers a restart when one is staged."
        info "Uninstall: rm ~/.local/bin/Schemaic.AppImage \\"
        info "              ~/.local/share/applications/${APP_ID}.desktop"
        ;;
esac
printf '%sSchemaic is in active development - do not trust it with data you care about.%s\n' "$YELLOW" "$RESET"
