#!/usr/bin/env bash
set -euo pipefail
[[ -f /.dockerenv && $EUID == 0 ]] || { echo 'Use native.sh, not this script on the host.' >&2; exit 1; }
format=${1:?}
case "$format" in
    deb)
        export DEBIAN_FRONTEND=noninteractive
        apt-get update
        apt-get install -y --no-install-recommends /output/packages/biri_*_amd64.deb
        dpkg --verify biri
        ;;
    rpm)
        dnf install -y --setopt=install_weak_deps=False /output/packages/biri-[0-9]*.x86_64.rpm
        rpm --verify biri
        ;;
    tumbleweed)
        # The container's /usr/etc policy omits %doc payloads; validate the full package.
        mkdir -p /etc/zypp/zypp.conf.d
        printf 'rpm.install.excludedocs = no\n' > /etc/zypp/zypp.conf.d/excludedocs.conf
        zypper --non-interactive refresh
        zypper --non-interactive dup --no-recommends
        zypper --non-interactive install --no-recommends --allow-unsigned-rpm /output/packages/biri-[0-9]*.x86_64.rpm
        rpm --verify biri
        ;;
    arch)
        sed -i 's@usr/share/doc/\*@@g' /etc/pacman.conf
        pacman -Syu --noconfirm
        pacman -U --noconfirm /output/packages/biri-[0-9]*-x86_64.pkg.tar.zst
        pacman -Qkk biri
        ;;
    void)
        xbps-install -Syu xbps
        xbps-install -yu
        xbps-install -y -R /output/packages biri
        xbps-pkgdb biri
        ;;
    *) exit 1 ;;
esac
# Do these checks before installing test tools, which could mask missing dependencies.
libraries=$(ldd /usr/bin/niri)
printf '%s\n' "$libraries"
if grep -Fq 'not found' <<< "$libraries"; then exit 1; fi
/usr/bin/niri --version
/usr/bin/niri validate --config /usr/share/doc/biri/default-config.kdl
for file in /usr/bin/niri-session /usr/share/wayland-sessions/niri.desktop \
    /usr/share/xdg-desktop-portal/niri-portals.conf /usr/share/licenses/biri/LICENSE \
    /usr/share/doc/biri/shaders/README.md /usr/share/bash-completion/completions/niri \
    /usr/share/fish/vendor_completions.d/niri.fish /usr/share/zsh/site-functions/_niri; do
    test -s "$file"
done
test -x /usr/bin/niri-session
if [[ $format == void ]]; then
    test ! -e /usr/lib/systemd/user/niri.service
    command -v dbus-run-session
else
    test -s /usr/lib/systemd/user/niri.service
    test -s /usr/lib/systemd/user/niri-shutdown.target
fi
case "$format" in
    deb) apt-get install -y --no-install-recommends python3 desktop-file-utils libgl1-mesa-dri dbus-daemon ;;
    rpm) dnf install -y python3 desktop-file-utils mesa-dri-drivers dbus-daemon ;;
    tumbleweed) zypper --non-interactive install --no-recommends python3 desktop-file-utils Mesa-dri dbus-1-daemon ;;
    arch) pacman -S --noconfirm --needed python desktop-file-utils mesa dbus ;;
    void) xbps-install -y python3 desktop-file-utils mesa-dri dbus ;;
esac
# DesktopNames is a display-manager session extension, not an application key.
# Verify it explicitly, then validate the remaining standard desktop-entry fields.
grep -Fxq 'DesktopNames=niri' /usr/share/wayland-sessions/niri.desktop
sed '/^DesktopNames=/d' /usr/share/wayland-sessions/niri.desktop > /tmp/biri-session.desktop
desktop-file-validate /tmp/biri-session.desktop
python3 /source/scripts/packaging/check-installed.py /output/source.json
