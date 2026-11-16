#!/usr/bin/env python3
"""Export HEAD and vendor its locked Rust dependencies into a shared source bundle."""
import datetime
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import tomllib


def export(output):
    root = Path(subprocess.check_output(['git', 'rev-parse', '--show-toplevel'], text=True).strip())
    subprocess.run(['git', 'diff', '--exit-code', 'HEAD', '--'], cwd=root, check=True)
    output = output.resolve()
    if output == root or root in output.parents:
        raise ValueError('Output must be outside the checkout')
    output.mkdir(parents=True, exist_ok=True)
    if any(output.iterdir()):
        raise ValueError('Output must be empty')
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    epoch = int(subprocess.check_output(['git', 'show', '-s', '--format=%ct', 'HEAD'], cwd=root))
    with tempfile.TemporaryDirectory(prefix='biri-source-') as work:
        work = Path(work)
        archive = work / 'checkout.tar'
        subprocess.run(['git', 'archive', '--prefix=biri-checkout/', '-o', str(archive), 'HEAD'], cwd=root, check=True)
        with tarfile.open(archive) as source:
            source.extractall(work, filter='data')
        stage = work / 'biri-checkout'
        version = tomllib.loads((stage / 'Cargo.toml').read_text())['workspace']['package']['version']
        identity = {
            'commit': commit,
            'version': version,
            'epoch': epoch,
            'timestamp': datetime.datetime.fromtimestamp(epoch, datetime.timezone.utc).strftime('%Y%m%d%H%M%S'),
        }
        (stage / '.biri-source.json').write_text(json.dumps(identity, indent=2) + '\n')
        (stage / '.cargo').mkdir(exist_ok=True)
        with (stage / '.cargo/config.toml').open('w') as config:
            subprocess.run(['cargo', 'vendor', '--locked', '--versioned-dirs', 'vendor'], cwd=stage, stdout=config, check=True)
        subprocess.run([
            'tar', '--sort=name', f'--mtime=@{epoch}', '--owner=0', '--group=0', '--numeric-owner',
            '-czf', str(output / 'source.tar.gz'), '-C', str(work), 'biri-checkout',
        ], check=True)
    with (output / 'source.tar.gz').open('rb') as stream:
        identity['sha256'] = hashlib.file_digest(stream, 'sha256').hexdigest()
    (output / 'source.json').write_text(json.dumps(identity, indent=2) + '\n')
    print(json.dumps(identity, indent=2))


if __name__ == '__main__':
    export(Path(sys.argv[1]))
