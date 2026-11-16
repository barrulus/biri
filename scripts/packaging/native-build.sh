#!/usr/bin/env bash
set -euo pipefail
[[ -f /.dockerenv && $EUID == 0 ]] || { echo 'Use native.sh, not this script on the host.' >&2; exit 1; }
format=${1:?}
case "$format" in
    deb)
        export DEBIAN_FRONTEND=noninteractive
        apt-get update
        apt-get install -y --no-install-recommends build-essential debhelper devscripts python3 ca-certificates curl xz-utils
        ;;
    rpm) dnf install -y rpm-build rpmdevtools python3 tar gzip curl ca-certificates xz ;;
    tumbleweed)
        zypper --non-interactive refresh
        zypper --non-interactive dup --no-recommends
        zypper --non-interactive install --no-recommends rpm-build python3 tar gzip curl ca-certificates xz shadow
        ;;
    arch) pacman -Syu --noconfirm --needed base-devel python curl ca-certificates xz ;;
    void)
        xbps-install -Syu xbps
        xbps-install -yu
        xbps-install -y python3 curl ca-certificates xz
        ;;
    *) exit 1 ;;
esac
python3 /source/scripts/packaging/verify-source.py /input /build
bash /source/scripts/packaging/install-rust.sh
version=$(cat /build/version)
snapshot=$(cat /build/snapshot)
checksum=$(cat /build/sha256)
mkdir -p /output/packages /output/recipe
case "$format" in
    deb)
        mv /build/biri-checkout "/build/biri-$version+git$snapshot"
        cd "/build/biri-$version+git$snapshot"
        cp -a packaging/debian debian
        chmod +x debian/rules
        DEBFULLNAME='Biri maintainers' DEBEMAIL='barrulus@users.noreply.github.com' \
            dch --newversion "$version+git$snapshot-1" --distribution unstable 'Build exact CI source snapshot.'
        apt-get -y --no-install-recommends build-dep .
        cp -a debian /output/recipe/
        cp /input/source.tar.gz "/build/biri_${version}+git${snapshot}.orig.tar.gz"
        dpkg-buildpackage -S -us -uc
        mkdir -p /output/debian-source /build/rebuild
        cp /build/*.dsc /build/*.orig.tar.gz /build/*.debian.tar.xz \
            /build/*_source.changes /build/*_source.buildinfo /output/debian-source/
        dpkg-source -x /output/debian-source/*.dsc /build/rebuild/biri
        cd /build/rebuild/biri
        DEB_BUILD_OPTIONS="parallel=${CI_JOBS:-2}" dpkg-buildpackage -b -us -uc
        cp /build/rebuild/*.deb /build/rebuild/*.changes /build/rebuild/*.buildinfo /output/packages/
        ;;
    rpm|tumbleweed)
        mkdir -p /build/rpm/{SOURCES,SPECS}
        cp /input/source.tar.gz /build/rpm/SOURCES/biri-source.tar.gz
        spec=/build/rpm/SPECS/biri.spec
        cp /build/biri-checkout/packaging/rpm/biri.spec "$spec"
        sed -i -e "s/^Version:.*/Version:        $version/" \
            -e "s/^Release:.*/Release:        1.$snapshot%{?dist}/" "$spec"
        rpmspec -q --buildrequires "$spec" > /build/dependencies
        mapfile -t dependencies < /build/dependencies
        if [[ $format == rpm ]]; then
            dnf install -y "${dependencies[@]}"
        else
            zypper --non-interactive install --no-recommends "${dependencies[@]}"
        fi
        rpmbuild -ba --define '_topdir /build/rpm' --define "_smp_mflags -j${CI_JOBS:-2}" "$spec"
        find /build/rpm/RPMS /build/rpm/SRPMS -name '*.rpm' -exec cp '{}' /output/packages/ \;
        cp "$spec" /output/recipe/
        ;;
    arch)
        mkdir -p /build/arch
        cp /build/biri-checkout/packaging/arch/PKGBUILD /build/arch/
        cp /input/source.tar.gz /build/arch/biri-source.tar.gz
        sed -i -e "s/^pkgver=.*/pkgver=$version.r$snapshot/" \
            -e "s/CHECKOUT_SHA256/$checksum/" /build/arch/PKGBUILD
        useradd --create-home builder
        chown -R builder:builder /build/arch
        cd /build/arch
        runuser -u builder -- makepkg --printsrcinfo > /build/SRCINFO
        mapfile -t dependencies < <(awk '$1 == "depends" || $1 == "makedepends" {print $3}' /build/SRCINFO)
        pacman -S --noconfirm --needed "${dependencies[@]}"
        runuser -u builder -- env CI_JOBS="${CI_JOBS:-2}" makepkg --noconfirm
        cp /build/arch/*.pkg.tar.zst /output/packages/
        cp /build/arch/PKGBUILD /build/SRCINFO /output/recipe/
        ;;
    void)
        source /source/scripts/packaging/environment
        export PATH="/usr/libexec/chroot-git:$PATH"
        mkdir /buildroot
        cd /buildroot
        git init
        git remote add origin https://github.com/void-linux/void-packages.git
        git fetch --depth 1 origin "$VOID_PACKAGES_REV"
        git checkout --detach FETCH_HEAD
        ln -s / masterdir
        xbps-uhelper arch > /.xbps_chroot_init
        export XBPS_CHROOT_CMD=ethereal XBPS_ALLOW_CHROOT_BREAKOUT=yes
        export XBPS_MAKEJOBS=${CI_JOBS:-2}
        mkdir -p srcpkgs/biri
        cp /build/biri-checkout/packaging/void/biri/template srcpkgs/biri/template
        # Cache-only URL: a missing source archive must fail instead of fetching unrelated source.
        void_version=$version.${snapshot%%.git*}
        archive=biri-$void_version.tar.gz
        sed -i -e "s/^version=.*/version=$void_version/" -e "s/CHECKOUT_SHA256/$checksum/" \
            -e "s@^distfiles=.*@distfiles=\"https://invalid.invalid/local-checkout/$archive\"@" srcpkgs/biri/template
        printf '\nwrksrc="biri-checkout"\n' >> srcpkgs/biri/template
        mkdir -p "hostdir/sources/biri-$void_version"
        cp /input/source.tar.gz "hostdir/sources/biri-$void_version/$archive"
        ./xbps-src pkg biri
        cp hostdir/binpkgs/*.xbps hostdir/binpkgs/*-repodata /output/packages/
        cp srcpkgs/biri/template /output/recipe/
        ;;
esac
cp /input/source.json /output/source.json
