#!/usr/bin/env bash
set -euo pipefail
source scripts/packaging/environment
if [[ -d /opt/rustup/toolchains/$RUST_VERSION-x86_64-unknown-linux-gnu/bin ]]; then
    export PATH="/opt/rustup/toolchains/$RUST_VERSION-x86_64-unknown-linux-gnu/bin:$PATH"
fi
export CARGO_HOME="$PWD/.cargo-home" CARGO_TARGET_DIR="$PWD/target"
export NIRI_BUILD_COMMIT
NIRI_BUILD_COMMIT=$(python3 -c 'import json; print(json.load(open(".biri-source.json"))["commit"][:7])')
args=(--release --frozen --package niri --bin niri --jobs "${CI_JOBS:-2}")
if [[ ${1:-systemd} == void ]]; then
    args+=(--no-default-features --features 'dbus,xdp-gnome-screencast')
fi
cargo build "${args[@]}"
