#!/usr/bin/env python3
"""Collect only matching, successfully installed native packages for publication."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import tarfile

from debian_source import debian_source_files, only

ASSETS = {
    'deb': ('biri_*_amd64.deb', 'biri-debian13-amd64.deb'),
    'rpm': ('biri-[0-9]*.x86_64.rpm', 'biri-fedora44-x86_64.rpm'),
    'arch': ('biri-[0-9]*-x86_64.pkg.tar.zst', 'biri-arch-x86_64.pkg.tar.zst'),
    'tumbleweed': ('biri-[0-9]*.x86_64.rpm', 'biri-tumbleweed-x86_64.rpm'),
}


def digest(path):
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def verify_packages(artifacts, identity):
    for kind in [*ASSETS, 'void']:
        root = artifacts / f'package-{kind}'
        receipt = json.loads((root / 'verified.json').read_text())
        if (receipt['source'] != identity or receipt['format'] != kind or receipt['clean_install'] is not True
                or json.loads((root / 'source.json').read_text()) != identity):
            raise ValueError(f'{kind}: package has not passed clean installation for this source')
        actual = {p.name: digest(p) for p in (root / 'packages').iterdir() if p.is_file()}
        if not actual or actual != receipt['packages']:
            raise ValueError(f'{kind}: package checksum mismatch')


def prepare(artifacts, source, output, commit, repository, run_id):
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Expected a full commit SHA')
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository) or not run_id.isdigit():
        raise ValueError('Invalid repository or run ID')
    identity = json.loads((source / 'source.json').read_text())
    if identity['commit'] != commit or digest(source / 'source.tar.gz') != identity['sha256']:
        raise ValueError('Source bundle does not match the expected revision')
    verify_packages(artifacts, identity)
    selected = {name: only(artifacts / f'package-{kind}' / 'packages', pattern)
                for kind, (pattern, name) in ASSETS.items()}
    void = artifacts / 'package-void' / 'packages'
    only(void, 'biri-[0-9]*.x86_64.xbps')
    if not (void / 'x86_64-repodata').is_file():
        raise ValueError('Void repository index is missing')
    debian = debian_source_files(artifacts / 'package-deb' / 'debian-source', identity)
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError('Downloads output must be empty')
    for name, path in selected.items():
        shutil.copyfile(path, output / name)
    with tarfile.open(output / 'biri-void-glibc-x86_64.tar.gz', 'w:gz') as archive:
        for path in sorted(void.iterdir()):
            archive.add(path, arcname=f'biri-void/packages/{path.name}')
    with tarfile.open(output / 'biri-debian-source.tar.gz', 'w:gz') as archive:
        for path in debian:
            archive.add(path, arcname=f'biri-debian-source/{path.name}')
    shutil.copyfile(source / 'source.tar.gz', output / 'biri-source.tar.gz')
    shutil.copyfile(source / 'source.json', output / 'source.json')
    (output / 'SHA256SUMS').write_text(''.join(f'{digest(p)}  {p.name}\n' for p in sorted(output.iterdir())))
    (output / 'release-notes.md').write_text(
        f'Built from [{commit[:12]}](https://github.com/{repository}/commit/{commit}).\n\n'
        f'[Build and installation checks](https://github.com/{repository}/actions/runs/{run_id}).\n\n'
        'Development snapshots for Void glibc, Debian 13, Arch Linux, Fedora 44, and openSUSE Tumbleweed '
        '(x86-64). These unsigned packages replace niri and retain its commands and configuration paths. '
        'They are not official distribution packages or stable releases.\n\n'
        f'[Installation instructions](https://github.com/{repository}/blob/{commit}/packaging/README.md). '
        'Download SHA256SUMS alongside the packages and verify with `sha256sum --check --ignore-missing SHA256SUMS`. '
        'The source bundle includes locked Cargo dependencies. Downloads are replaced as newer main builds pass.\n')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('artifacts', type=Path)
    parser.add_argument('source', type=Path)
    parser.add_argument('output', type=Path)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--repository', required=True)
    parser.add_argument('--run-id', required=True)
    args = parser.parse_args()
    prepare(args.artifacts, args.source, args.output, args.commit, args.repository, args.run_id)
