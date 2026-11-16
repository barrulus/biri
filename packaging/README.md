# Native packages

The [Packages workflow](https://github.com/barrulus/biri/actions/workflows/packages.yml)
builds x86-64 packages for Void Linux (glibc), Debian 13, Arch Linux, Fedora 44,
and openSUSE Tumbleweed. It follows the build and clean-install approach used by
[Rummage](https://github.com/barrulus/rummage/blob/main/contributing/ci-packaging.md).

## Downloads and installation

Download packages from [Latest development build](https://github.com/barrulus/biri/releases/tag/development).
These are unsigned development snapshots, not official distribution packages or
stable releases. Fedora and openSUSE have separate RPMs. Older Debian releases,
Ubuntu, Leap, RHEL, Void musl, and native ARM packages are not covered by this
matrix; the Nix workflow separately builds x86-64 and ARM64.

Save `SHA256SUMS` alongside the files you download and verify them:

```sh
sha256sum --check --ignore-missing SHA256SUMS
```

All packages are named **biri**, provide **niri**, and conflict with upstream niri
because they install the same commands and session files. Remove your existing
niri package when the package manager requests it. They retain `/usr/bin/niri`,
`niri-session`, `~/.config/niri/config.kdl`, the `niri` desktop identity, and the
existing portal integration. The display manager labels the session **Biri**.
Default configuration and bundled shaders are under `/usr/share/doc/biri/`.

Install the package for your distribution:

```sh
# Debian 13
sudo apt install ./biri-debian13-amd64.deb

# Fedora 44
sudo dnf install ./biri-fedora44-x86_64.rpm

# Arch Linux: update the system first, avoiding partial upgrades.
sudo pacman -Syu
sudo pacman -U ./biri-arch-x86_64.pkg.tar.zst

# openSUSE Tumbleweed: update first. Allow only this unsigned local RPM.
sudo zypper dup
sudo zypper install --allow-unsigned-rpm ./biri-tumbleweed-x86_64.rpm

# Void glibc: keep the package and repository index together.
tar -xzf biri-void-glibc-x86_64.tar.gz
sudo xbps-install -Syu
sudo xbps-install -R "$PWD/biri-void/packages" biri
```

The Void package builds without systemd integration and supplies a session
launcher using `dbus-run-session` when needed. It does not install systemd user
units. Other packages include the upstream systemd session launcher and units.
Install the desktop portals and Xwayland integration described in
[Packaging niri](../docs/wiki/Packaging-niri.md#recommended-dependencies) for
screencasting, file dialogs, secrets, and X11 applications.

## What CI verifies

Every PR, push to `main`, and manual run builds the exact checked-out commit.
A source job vendors the dependencies selected by `Cargo.lock`, then records the
commit, package version, UTC commit timestamp, and archive SHA-256 in `source.json`.
The same source archive goes to every builder; a checksum mismatch stops the job.
Snapshot package versions include the commit timestamp (and hash where the package
format permits), while `niri --version` reports the original application version
and source commit.

The builders use `dpkg-buildpackage`, `rpmbuild`, `makepkg`, and `xbps-src` with the
checked-in recipes. Debian binaries are built from an unpacked `3.0 (quilt)` source
package. Arch builds as an unprivileged user. Void's ethereal build mode runs only
inside a disposable Docker container; the containers do not use privileged mode.

Each package is installed through the native package manager in a second, fresh
container. Before adding test fixtures, CI checks package integrity, linked
libraries, the installed binary, configuration, session files, shaders, and
completions. Then it starts the installed compositor headlessly, checks its output
through IPC, and requests a clean shutdown. This checks startup and IPC, not a
physical GPU/display session. The existing CI and Nix workflows run unit tests.

Successful installation creates a receipt binding package checksums to the source
identity. Download preparation requires all five receipts to agree and checks
their payloads. PRs retain artifacts for 14 days and cannot publish. On trusted
`main` runs, publication also waits for both the regular CI and Nix workflows to
pass for the same commit, then updates the `development` prerelease.

Older reruns cannot roll the prerelease back over a newer commit. Failed checks
leave existing downloads untouched. GitHub asset replacement is not atomic;
checksums are uploaded last, so retry if verification fails during an update.
The prerelease must remain mutable. No signing secrets or package-repository
accounts are needed for these local packages.

## Reproducing a build

Host requirements: Docker, Git, Python 3.12+, GNU tar, and Cargo matching the pinned
Rust version in [environment](../scripts/packaging/environment). Commit the intended
source first. Untracked files are excluded by `git archive`; tracked edits are
rejected so package metadata cannot misidentify the checkout.

```sh
python3 -m unittest discover -s scripts/packaging/tests -v
python3 scripts/packaging/export-source.py /tmp/biri-package-source
bash scripts/packaging/native.sh deb /tmp/biri-package-source /tmp/biri-debian
# Other targets: rpm, arch, tumbleweed, void. Use a separate empty output directory.
```

`CI_JOBS` controls build parallelism (default 2). Run `native.sh`, not its container
entry points, on the host. The output contains packages, the effective recipe,
source identity, and a successful-installation receipt. Debian also includes its
source package. CI retains build logs even on failure.

Base images and the Void source-packages revision are pinned, as is Rust. The Rust
bootstrap is necessary because Debian 13's packaged compiler is older than Biri's
minimum supported version. Binary dependency repositories still update, so this
does not claim bit-for-bit reproducibility. The source bundle includes the vendor
directory and its upstream license files. Toolchain installation and native build
dependencies need network access; compiling Rust dependencies is offline.

The root [`PKGBUILD`](../PKGBUILD) remains the existing `biri-git` VCS recipe.
[`arch/PKGBUILD`](arch/PKGBUILD) is the distinct, checksum-verified CI snapshot
recipe. These recipes are not submissions to distribution repositories.
