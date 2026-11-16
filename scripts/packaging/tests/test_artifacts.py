import importlib.util
import io
import json
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(SCRIPTS))


def module(name):
    spec = importlib.util.spec_from_file_location(name, SCRIPTS / f'{name}.py')
    result = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(result)
    return result


source = module('verify-source')
downloads = module('prepare-downloads')


class ArtifactTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.identity = {'commit': 'a' * 40, 'version': '26.4.0', 'timestamp': '20261116000000',
                         'epoch': 1794787200}

    def bundle(self, member='biri-checkout/.biri-source.json'):
        directory = self.root / 'source'
        directory.mkdir()
        with tarfile.open(directory / 'source.tar.gz', 'w:gz') as archive:
            data = json.dumps(self.identity).encode()
            entry = tarfile.TarInfo(member)
            entry.size = len(data)
            archive.addfile(entry, io.BytesIO(data))
        self.identity['sha256'] = downloads.digest(directory / 'source.tar.gz')
        (directory / 'source.json').write_text(json.dumps(self.identity))
        return directory

    def receipts(self):
        for kind in [*downloads.ASSETS, 'void']:
            root = self.root / f'package-{kind}'
            (root / 'packages').mkdir(parents=True)
            payload = root / 'packages/package'
            payload.write_text(kind)
            (root / 'source.json').write_text(json.dumps(self.identity))
            (root / 'verified.json').write_text(json.dumps({
                'source': self.identity, 'format': kind, 'clean_install': True,
                'packages': {'package': downloads.digest(payload)},
            }))

    def test_exact_source_bundle_is_extracted(self):
        directory = self.bundle()
        self.assertEqual(source.verify(directory, self.root / 'build'), self.identity)
        self.assertEqual((self.root / 'build/commit').read_text(), 'a' * 40)

    def test_source_checksum_mismatch_is_rejected_before_extraction(self):
        directory = self.bundle()
        with (directory / 'source.tar.gz').open('ab') as archive:
            archive.write(b'changed')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            source.verify(directory, self.root / 'build')
        self.assertFalse((self.root / 'build').exists())

    def test_archive_path_escape_is_rejected(self):
        directory = self.bundle('../escape')
        with self.assertRaisesRegex(ValueError, 'root'):
            source.verify(directory, self.root / 'build')
        self.assertFalse((self.root / 'escape').exists())

    def test_matching_installation_receipts_are_accepted(self):
        self.receipts()
        downloads.verify_packages(self.root, self.identity)

    def test_changed_package_is_rejected(self):
        self.receipts()
        (self.root / 'package-deb/packages/package').write_text('tampered')
        with self.assertRaisesRegex(ValueError, 'checksum'):
            downloads.verify_packages(self.root, self.identity)

    def test_mixed_source_revisions_are_rejected(self):
        self.receipts()
        path = self.root / 'package-void/verified.json'
        receipt = json.loads(path.read_text())
        receipt['source']['commit'] = 'b' * 40
        path.write_text(json.dumps(receipt))
        with self.assertRaisesRegex(ValueError, 'source'):
            downloads.verify_packages(self.root, self.identity)

    def test_failed_installation_cannot_be_published(self):
        self.receipts()
        path = self.root / 'package-rpm/verified.json'
        receipt = json.loads(path.read_text())
        receipt['clean_install'] = False
        path.write_text(json.dumps(receipt))
        with self.assertRaisesRegex(ValueError, 'installation'):
            downloads.verify_packages(self.root, self.identity)


if __name__ == '__main__':
    unittest.main()
