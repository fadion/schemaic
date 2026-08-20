# RPM spec for an already-built Schemaic binary. Driven by build-rpm.sh, which
# stages the payload and passes the version in; see that script for why this is
# packaged from a prebuilt binary rather than compiled here.
%global appid io.github.fadion.Schemaic

# There is no debuginfo to extract. The release binary is produced by
# cargo-zigbuild with the default release profile, and carries neither DWARF
# sections nor a symbol table (`readelf -S` shows no .debug_* and no .symtab),
# so leaving the debuginfo machinery on would fail the build over an empty
# package rather than produce anything useful.
%global debug_package %{nil}

# A fallback so the spec parses when read on its own; build-rpm.sh always passes
# the real one with --define.
%{!?schemaic_version: %global schemaic_version 0.0.0}

Name:           schemaic
Version:        %{schemaic_version}
Release:        1%{?dist}
Summary:        Fast, native SQL editor for MySQL, MariaDB, PostgreSQL and SQLite

License:        MIT
URL:            https://github.com/fadion/schemaic

# Every one of these is loaded with dlopen, not linked, so rpm's automatic
# dependency generator cannot see any of them - it reads ELF DT_NEEDED entries,
# and the binary's list is glibc and nothing else. winit reaches X11, Wayland
# and xkbcommon through libloading, and wgpu reaches Vulkan and EGL the same
# way. Without these lines the package installs cleanly and then cannot open a
# window.
#
# Written as *sonames* rather than package names on purpose. Every rpm
# distribution auto-provides `libfoo.so.N()(64bit)` for the package that ships
# that library, so one spec resolves correctly on Fedora, RHEL and openSUSE
# alike - which matters, because those three disagree on nearly every one of
# these package names (Fedora's libX11 is openSUSE's libX11-6). The .deb has no
# such option: dpkg has no soname provides, so build-deb.sh hard-codes Debian
# package names instead.
Requires:       libxkbcommon.so.0()(64bit)
Requires:       libxkbcommon-x11.so.0()(64bit)
Requires:       libwayland-client.so.0()(64bit)
Requires:       libwayland-egl.so.1()(64bit)
Requires:       libX11.so.6()(64bit)
Requires:       libX11-xcb.so.1()(64bit)
Requires:       libxcb.so.1()(64bit)
Requires:       libvulkan.so.1()(64bit)
Requires:       libEGL.so.1()(64bit)

# The Vulkan loader above is only the dispatch library; something has to
# implement it. Weak rather than hard because a machine may have a vendor
# driver instead, and because wgpu falls back to the GL backend.
Recommends:     mesa-vulkan-drivers

%description
Schemaic is a native SQL editor built to feel instant: a GPU-rendered interface
that scrolls 200k-row result sets smoothly, an editable results grid, visual
schema editing, and completion and diagnostics that follow the dialect you are
connected to.

It connects to MySQL, MariaDB, PostgreSQL and SQLite, directly or over an SSH
tunnel, and keeps credentials in the OS keyring. There is no account, no
telemetry and no cloud service.

Schemaic is in active development and should not be trusted with production
data.

%prep
# Nothing to unpack: build-rpm.sh stages the installed tree under
# %%{_sourcedir}/payload and %%install copies it in whole.

%build
# Nothing to build.

%install
cp -a %{_sourcedir}/payload/. %{buildroot}/

%files
%license %{_datadir}/doc/schemaic/LICENSE
%doc %{_datadir}/doc/schemaic/README.md
%doc %{_datadir}/doc/schemaic/THIRD-PARTY-NOTICES.md
%doc %{_datadir}/doc/schemaic/IBMPlex-OFL.txt
%doc %{_datadir}/doc/schemaic/Lucide-LICENSE.txt
%dir %{_datadir}/doc/schemaic
%{_bindir}/schemaic
%{_datadir}/applications/%{appid}.desktop
%{_datadir}/metainfo/%{appid}.metainfo.xml
%{_datadir}/icons/hicolor/512x512/apps/%{appid}.png

%changelog
# Intentionally empty. The release notes on the GitHub Release are the
# changelog, and a second copy maintained by hand here would only go stale.
