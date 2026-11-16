"""Validate the native Debian source package before publication."""
import hashlib


def only(directory, pattern):
    matches = list(directory.glob(pattern))
    if len(matches) != 1 or not matches[0].is_file():
        raise ValueError(f"Expected exactly one {pattern} in {directory}")
    return matches[0]


def debian_source_files(directory, identity):
    """Check the unsigned 3.0 (quilt) descriptor before publishing its components."""
    upstream = f"{identity['version']}+git{identity['timestamp']}.git{identity['commit'][:12]}"
    version = upstream + '-1'
    descriptor = only(directory, 'biri_*.dsc')
    fields = {}
    current = None
    for line in descriptor.read_text().splitlines():
        if line.startswith((' ', '\t')) and current:
            fields[current] += '\n' + line.strip()
        elif ':' in line:
            current, value = line.split(':', 1)
            if current in fields:
                raise ValueError('Duplicate Debian source descriptor field')
            fields[current] = value.strip()
        else:
            current = None
    if (fields.get('Source'), fields.get('Version'), fields.get('Format')) != ('biri', version, '3.0 (quilt)'):
        raise ValueError('Debian source descriptor does not match the snapshot')
    expected = {f'biri_{upstream}.orig.tar.gz', f'biri_{version}.debian.tar.xz'}
    found = set()
    components = []
    for line in fields.get('Checksums-Sha256', '').splitlines():
        if not line.strip():
            continue
        digest, size, name = line.split()
        if name not in expected or name in found:
            raise ValueError('Unexpected Debian source component')
        path = directory / name
        if path.stat().st_size != int(size) or hashlib.sha256(path.read_bytes()).hexdigest() != digest:
            raise ValueError('Debian source component checksum mismatch')
        if name.endswith('.orig.tar.gz') and digest != identity['sha256']:
            raise ValueError('Debian original source differs from the checkout archive')
        found.add(name)
        components.append(path)
    if found != expected:
        raise ValueError('Missing Debian source component')
    return [descriptor, *components,
            only(directory, 'biri_*_source.changes'),
            only(directory, 'biri_*_source.buildinfo')]
