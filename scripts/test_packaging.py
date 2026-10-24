"""Exercise archive integrity and installation boundaries with synthetic files."""
import hashlib
import io
import json
from pathlib import Path
import stat
import struct
import tarfile
import tempfile
import unittest
import zipfile

import package


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def fixture(self, target='x86_64-unknown-linux-gnu', extra=None, corrupt=False):
        prefix = package.archive_stem('0.1.0', target)
        windows = target.endswith('windows-msvc')
        binary = bytearray(128)
        if windows:
            binary[:2] = b'MZ'
            struct.pack_into('<I', binary, 60, 64)
            binary[64:68] = b'PE\0\0'
            struct.pack_into('<H', binary, 68, 0x8664)
        else:
            binary[:6] = b'\x7fELF\x02\x01'
            struct.pack_into('<H', binary, 18, 62)
        files = {f'bin/{name}' + ('.exe' if windows else ''): (bytes(binary), 0o755)
                 for name in ['hudson', 'hudson-cli', 'hudsonbox']}
        names = ['README.md', 'LICENSE', 'NOTICE', 'CHANGELOG.md', 'SECURITY.md', 'THIRD_PARTY_NOTICES.txt']
        names += [f'docs/{name}' for name in package.REQUIRED_DOCS]
        names += [f'completions/{name}' for name in package.SHELL_FILES.values()]
        files.update({name: (b'release fixture\n', 0o644) for name in names})
        manifest = {'schema': 1, 'version': '0.1.0', 'commit': 'a' * 40,
                    'dirty': False, 'target': target,
                    'files': {name: {'sha256': hashlib.sha256(data).hexdigest(),
                                     'size': len(data), 'mode': mode}
                              for name, (data, mode) in files.items()}}
        files['manifest.json'] = (json.dumps(manifest).encode(), 0o644)
        if corrupt:
            files['README.md'] = (b'tampered\n', 0o644)
        archive = self.root / package.archive_name('0.1.0', target)
        if windows:
            with zipfile.ZipFile(archive, 'w') as output:
                for name, (data, mode) in files.items():
                    entry = zipfile.ZipInfo(f'{prefix}/{name}')
                    entry.create_system = 3
                    entry.external_attr = (stat.S_IFREG | mode) << 16
                    output.writestr(entry, data)
                if extra:
                    output.writestr(extra, b'not a release file')
        else:
            with tarfile.open(archive, 'w:gz') as output:
                for name, (data, mode) in files.items():
                    entry = tarfile.TarInfo(f'{prefix}/{name}')
                    entry.size, entry.mode = len(data), mode
                    output.addfile(entry, io.BytesIO(data))
                if extra:
                    entry = tarfile.TarInfo(extra)
                    entry.size = 1
                    output.addfile(entry, io.BytesIO(b'x'))
        return archive

    def test_tar_and_zip_install_all_required_files(self):
        for target in ['x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc']:
            with self.subTest(target=target):
                archive = self.fixture(target)
                destination = self.root / target
                manifest = package.verify(archive, 'a' * 40, destination)
                self.assertEqual(manifest['target'], target)
                folder = destination / package.archive_stem('0.1.0', target)
                self.assertTrue((folder / 'README.md').is_file())
                self.assertTrue((folder / 'docs/install.md').is_file())
                with self.assertRaises(ValueError):
                    package.verify(archive, destination=destination)

    def test_corrupt_payload_is_rejected_before_writing(self):
        archive = self.fixture(corrupt=True)
        destination = self.root / 'install'
        with self.assertRaises(ValueError):
            package.verify(archive, destination=destination)
        self.assertFalse(destination.exists())

    def test_wrong_commit_and_renamed_archive_are_rejected(self):
        archive = self.fixture()
        with self.assertRaises(ValueError):
            package.verify(archive, 'b' * 40)
        renamed = archive.with_name('wrong.tar.gz')
        archive.rename(renamed)
        with self.assertRaises(ValueError):
            package.verify(renamed)

    def test_traversal_and_unlisted_members_are_rejected(self):
        for target, extra in [('x86_64-unknown-linux-gnu', '../escape'),
                              ('x86_64-pc-windows-msvc', 'C:/escape'),
                              ('x86_64-unknown-linux-gnu', 'unlisted')]:
            with self.subTest(extra=extra):
                archive = self.fixture(target, extra=extra)
                with self.assertRaises(ValueError):
                    package.verify(archive, destination=self.root / 'install')
                archive.unlink()

    def test_binary_target_is_checked(self):
        with self.assertRaises(ValueError):
            package.binary_header(b'not a native executable', 'x86_64-unknown-linux-gnu')
        binary = bytearray(64)
        binary[:6] = b'\x7fELF\x02\x01'
        struct.pack_into('<H', binary, 18, 183)
        with self.assertRaises(ValueError):
            package.binary_header(bytes(binary), 'x86_64-unknown-linux-gnu')


if __name__ == '__main__':
    unittest.main()
