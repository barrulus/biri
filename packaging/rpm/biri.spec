Name:           biri
Version:        26.4.0
Release:        1%{?dist}
Summary:        Scrollable-tiling Wayland compositor with Biri enhancements
License:        GPL-3.0-or-later
URL:            https://github.com/barrulus/biri
Source0:        biri-source.tar.gz
BuildRequires:  gcc
BuildRequires:  clang
BuildRequires:  cargo
BuildRequires:  rust >= 1.88
BuildRequires:  python3
BuildRequires:  pkgconfig
BuildRequires:  pkgconfig(libudev)
BuildRequires:  pkgconfig(gbm)
BuildRequires:  pkgconfig(xkbcommon)
BuildRequires:  pkgconfig(egl)
BuildRequires:  pkgconfig(wayland-server)
BuildRequires:  pkgconfig(libinput)
BuildRequires:  pkgconfig(dbus-1)
BuildRequires:  pkgconfig(libsystemd)
BuildRequires:  pkgconfig(libseat)
BuildRequires:  pkgconfig(libpipewire-0.3)
BuildRequires:  pkgconfig(pango)
BuildRequires:  pkgconfig(cairo)
BuildRequires:  pkgconfig(cairo-gobject)
BuildRequires:  pkgconfig(libdisplay-info)
Requires:       libwayland-server.so.0()(64bit)
Requires:       libEGL.so.1()(64bit)
Requires:       /usr/bin/dbus-update-activation-environment
Requires:       /usr/bin/ps
Requires:       /usr/bin/systemctl
Recommends:     xdg-desktop-portal-gnome
Recommends:     xdg-desktop-portal-gtk
Recommends:     gnome-keyring
Provides:       niri = %{version}
Conflicts:      niri

# Keep the upstream line-table debug information for useful panic backtraces.
%global debug_package %{nil}
%global __strip /bin/true

%description
Biri extends niri with vertical scrolling, post-process shaders, animated
focus rings and a multi-monitor carousel overview. It retains niri's command
names and configuration paths.

%prep
%autosetup -n biri-checkout

%build
bash scripts/packaging/build.sh systemd

%install
bash scripts/packaging/install.sh %{buildroot} systemd

%files
%{_bindir}/niri
%{_bindir}/niri-session
%{_datadir}/wayland-sessions/niri.desktop
%{_datadir}/xdg-desktop-portal/niri-portals.conf
%{_prefix}/lib/systemd/user/niri.service
%{_prefix}/lib/systemd/user/niri-shutdown.target
%{_datadir}/bash-completion/completions/niri
%{_datadir}/fish/vendor_completions.d/niri.fish
%{_datadir}/zsh/site-functions/_niri
%doc %{_datadir}/doc/biri/
%license %{_datadir}/licenses/biri/
