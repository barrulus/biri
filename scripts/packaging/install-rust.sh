#!/usr/bin/env bash
# Container-only bootstrap; Debian 13's packaged rustc predates Biri's MSRV.
set -euo pipefail
[[ -f /.dockerenv && $EUID == 0 ]] || exit 1
source /source/scripts/packaging/environment
curl --fail --silent --show-error --location --retry 5 \
    https://static.rust-lang.org/rustup/dist/x86_64-unknown-linux-gnu/rustup-init -o /tmp/biri-rustup-init
chmod +x /tmp/biri-rustup-init
RUSTUP_HOME=/opt/rustup CARGO_HOME=/opt/cargo RUSTUP_INIT_SKIP_PATH_CHECK=yes \
    /tmp/biri-rustup-init -y --no-modify-path --profile minimal --default-toolchain "$RUST_VERSION"
chmod -R a+rX /opt/rustup
