#!/usr/bin/env bash
# Shared package payload; runs from the extracted source directory.
set -euo pipefail
dest=${1:?usage: install.sh DESTDIR systemd|void}
session=${2:?}
install -Dm755 target/release/niri "$dest/usr/bin/niri"
install -Dm644 resources/niri.desktop "$dest/usr/share/wayland-sessions/niri.desktop"
sed -i 's/^Name=Niri$/Name=Biri/' "$dest/usr/share/wayland-sessions/niri.desktop"
install -Dm644 resources/niri-portals.conf "$dest/usr/share/xdg-desktop-portal/niri-portals.conf"
install -Dm644 resources/default-config.kdl "$dest/usr/share/doc/biri/default-config.kdl"
install -Dm644 LICENSE "$dest/usr/share/licenses/biri/LICENSE"
cp -R resources/shaders "$dest/usr/share/doc/biri/shaders"
find "$dest/usr/share/doc/biri/shaders" -type d -exec chmod 755 {} +
find "$dest/usr/share/doc/biri/shaders" -type f -exec chmod 644 {} +
chmod 755 "$dest/usr/share/doc/biri/shaders/scripts/"*
if [[ $session == void ]]; then
    install -Dm755 packaging/void/niri-session "$dest/usr/bin/niri-session"
else
    install -Dm755 resources/niri-session "$dest/usr/bin/niri-session"
    install -Dm644 resources/niri.service "$dest/usr/lib/systemd/user/niri.service"
    install -Dm644 resources/niri-shutdown.target "$dest/usr/lib/systemd/user/niri-shutdown.target"
fi
mkdir -p "$dest/usr/share/bash-completion/completions" "$dest/usr/share/fish/vendor_completions.d" \
    "$dest/usr/share/zsh/site-functions"
target/release/niri completions bash > "$dest/usr/share/bash-completion/completions/niri"
target/release/niri completions fish > "$dest/usr/share/fish/vendor_completions.d/niri.fish"
target/release/niri completions zsh > "$dest/usr/share/zsh/site-functions/_niri"
