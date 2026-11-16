#!/usr/bin/env python3
"""Verify the shared bundle before invoking a distribution's packaging tools."""
import hashlib
import json
from pathlib import Path
import re
import sys
import tarfile


def verify(source, output):
    identity = json.loads((source / 'source.json').read_text())
    for key, pattern in [('commit', r'[0-9a-f]{40}'), ('version', r'[0-9]+\.[0-9]+\.[0-9]+'),
                         ('timestamp', r'[0-9]{14}'), ('sha256', r'[0-9a-f]{64}')]:
        if not re.fullmatch(pattern, identity[key]):
            raise ValueError(f'Invalid source {key}')
    with (source / 'source.tar.gz').open('rb') as stream:
        checksum = hashlib.file_digest(stream, 'sha256').hexdigest()
    if checksum != identity['sha256']:
        raise ValueError('Source archive checksum mismatch')
    output.mkdir(parents=True, exist_ok=True)
    with tarfile.open(source / 'source.tar.gz') as archive:
        if any(not p.name.startswith('biri-checkout/') and p.name != 'biri-checkout' for p in archive):
            raise ValueError('Unexpected source archive root')
        archive.extractall(output, filter='data')
    embedded = json.loads((output / 'biri-checkout/.biri-source.json').read_text())
    if embedded != {k: v for k, v in identity.items() if k != 'sha256'}:
        raise ValueError('Embedded source identity mismatch')
    for key in ('version', 'commit', 'sha256'):
        (output / key).write_text(identity[key])
    (output / 'snapshot').write_text(identity['timestamp'] + '.git' + identity['commit'][:12])
    return identity


if __name__ == '__main__':
    verify(Path(sys.argv[1]), Path(sys.argv[2]))
