"""Exercise archive integrity and installation boundaries with synthetic files."""
import binascii
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import stat
import struct
import tarfile
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import zipfile

import package


class PackagingTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def fixture(self, target='x86_64-unknown-linux-gnu', extra=None, corrupt=False, additional_files=None):
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
        files.update(additional_files or {})
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

    def test_unsafe_paths_are_rejected_before_installation(self):
        for target in ('x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc'):
            for name in ('COM\u00b9.md', 'com\u00b2.notes.md', 'LPT\u00b3.md',
                         'NUL .md', 'CONIN$.md', 'CONOUT$/guide.md',
                         'guide\x1b.md', 'guide\u202e.md'):
                with self.subTest(target=target, name=name):
                    archive = self.fixture(target, additional_files={f'docs/{name}': (b'fixture\n', 0o644)})
                    destination = self.root / 'install'
                    with self.assertRaises(ValueError):
                        package.verify(archive)
                    with self.assertRaises(ValueError):
                        package.verify(archive, destination=destination)
                    self.assertFalse(destination.exists())
                    archive.unlink()

    def test_nonportable_archive_components_are_rejected(self):
        for name in ('docs/guide?.md', 'docs/guide*.md', 'docs/a|b.md',
                     'docs/a<b.md', 'docs/a>b.md', 'docs/a"b.md',
                     'docs/guide\x1b.md', 'docs/guide\n.md', 'docs/guide\x85.md',
                     'docs/guide\u202e.md', 'docs/guide\u2028.md'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                package.safe_name(name)

    def test_original_zip_names_are_validated_before_normalization(self):
        for name, raw_name in [('docs/guide.mdXnote', b'docs/guide.md\0note'),
                               ('docs/guide.md', b'docs\\guide.md')]:
            archive = self.root / 'normalized.zip'
            with zipfile.ZipFile(archive, 'w') as output:
                output.writestr(name, b'fixture')
            data = archive.read_bytes()
            self.assertEqual(data.count(name.encode()), 2)
            archive.write_bytes(data.replace(name.encode(), raw_name))
            with self.subTest(name=raw_name), self.assertRaises(ValueError):
                package.read_archive(archive)

    def test_portable_unicode_and_device_prefixes_remain_valid(self):
        for name in ('docs/COM10.md', 'docs/LPT0-guide.md', 'docs/auxiliary.md',
                     'docs/guide with spaces.md', 'docs/caf\u00e9.md', 'docs/\u6307\u5357.md'):
            with self.subTest(name=name):
                self.assertEqual(str(package.safe_name(name)), name)

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

    def test_archive_metadata_is_bounded_before_parsing(self):
        zipped = self.root / 'metadata.zip'
        entry = zipfile.ZipInfo('guide.md')
        entry.extra = struct.pack('<HH', 0xffff, 2048) + b'x' * 2048
        with zipfile.ZipFile(zipped, 'w') as output:
            output.writestr(entry, b'fixture')
        tarred = self.root / 'metadata.tar.gz'
        with tarfile.open(tarred, 'w:gz', format=tarfile.PAX_FORMAT) as output:
            entry = tarfile.TarInfo('guide.md')
            entry.size = 7
            entry.pax_headers = {'comment': 'x' * 2048}
            output.addfile(entry, io.BytesIO(b'fixture'))
        for archive in (zipped, tarred):
            with self.subTest(archive=archive.name), \
                    patch.object(package, 'MAX_METADATA_BYTES', 1024, create=True):
                with self.assertRaises(ValueError):
                    package.read_archive(archive)

    def test_oversized_manifests_are_rejected_before_payload_reads(self):
        content = b'{}' + b' ' * 1024
        zipped = self.root / 'manifest.zip'
        with zipfile.ZipFile(zipped, 'w') as output:
            output.writestr('root/manifest.json', content)
        tarred = self.root / 'manifest.tar.gz'
        with tarfile.open(tarred, 'w:gz') as output:
            entry = tarfile.TarInfo('root/manifest.json')
            entry.size = len(content)
            output.addfile(entry, io.BytesIO(content))
        for archive, stream_type in ((zipped, zipfile.ZipExtFile), (tarred, tarfile.ExFileObject)):
            with self.subTest(archive=archive.name), \
                    patch.object(package, 'MAX_MANIFEST_BYTES', 8, create=True), \
                    patch.object(stream_type, 'read', side_effect=AssertionError('oversized manifest read')):
                with self.assertRaises(ValueError):
                    package.read_archive(archive)

    def test_forged_tar_extension_size_never_reaches_the_decompressor(self):
        archive = self.root / 'forged-extension.tar.gz'
        extension = tarfile.TarInfo('pax')
        extension.type = tarfile.XHDTYPE
        extension.size = 1024 * 1024 * 1024
        with gzip.open(archive, 'wb') as output:
            output.write(extension.tobuf(format=tarfile.USTAR_FORMAT) + bytes(1024))
        read = gzip.GzipFile.read

        def bounded_read(stream, size=-1):
            self.assertGreaterEqual(size, 0, 'unbounded gzip metadata read')
            self.assertLessEqual(size, package.MAX_METADATA_BYTES,
                                 'oversized extension reached the decompressor')
            return read(stream, size)

        with patch.object(gzip.GzipFile, 'read', bounded_read), self.assertRaises(ValueError):
            package.read_archive(archive)

    def test_zip_local_extra_fields_can_differ_from_directory_metadata(self):
        archive = self.root / 'local-extra.zip'
        with zipfile.ZipFile(archive, 'w') as output:
            output.writestr('guide.md', b'fixture')
        data = bytearray(archive.read_bytes())
        directory = data.rindex(b'PK\x01\x02')
        end = data.rindex(b'PK\x05\x06')
        extra = struct.pack('<HH', 0xffff, 1024) + b'x' * 1024
        struct.pack_into('<H', data, 28, len(extra))
        struct.pack_into('<I', data, end + 16, directory + len(extra))
        name_length = struct.unpack_from('<H', data, 26)[0]
        data[30 + name_length:30 + name_length] = extra
        archive.write_bytes(data)
        self.assertEqual(package.read_archive(archive)['guide.md'][0], b'fixture')

    def test_ignored_directories_count_toward_the_archive_member_limit(self):
        for suffix in ('.zip', '.tar.gz'):
            for count in (2, 3):
                archive = self.root / ('directories' + suffix)
                if suffix == '.zip':
                    with zipfile.ZipFile(archive, 'w') as output:
                        for number in range(count):
                            output.writestr(f'directory-{number}/', b'')
                else:
                    with tarfile.open(archive, 'w:gz') as output:
                        for number in range(count):
                            entry = tarfile.TarInfo(f'directory-{number}')
                            entry.type = tarfile.DIRTYPE
                            output.addfile(entry)
                with self.subTest(suffix=suffix, count=count), \
                        patch.object(package, 'MAX_ARCHIVE_MEMBERS', 2, create=True):
                    if count == 2:
                        self.assertEqual(package.read_archive(archive), {})
                    else:
                        with self.assertRaises(ValueError):
                            package.read_archive(archive)

    def test_unsupported_zip_codecs_are_rejected(self):
        for codec in (zipfile.ZIP_BZIP2, zipfile.ZIP_LZMA):
            archive = self.root / 'codec.zip'
            with zipfile.ZipFile(archive, 'w', compression=codec) as output:
                output.writestr('guide.md', b'fixture')
            with self.subTest(codec=codec), self.assertRaises(ValueError):
                package.read_archive(archive)

    def test_zip_payload_reads_are_bounded_when_the_size_is_forged(self):
        archive = self.root / 'forged-size.zip'
        with zipfile.ZipFile(archive, 'w', compression=zipfile.ZIP_DEFLATED) as output:
            output.writestr('guide.md', b'x' * 65536)
        data = bytearray(archive.read_bytes())
        directory = data.rindex(b'PK\x01\x02')
        for crc_offset, size_offset in ((14, 22), (directory + 16, directory + 24)):
            struct.pack_into('<I', data, crc_offset, binascii.crc32(b'x'))
            struct.pack_into('<I', data, size_offset, 1)
        archive.write_bytes(data)
        read = zipfile.ZipExtFile.read

        def bounded_read(stream, size=-1):
            self.assertGreater(size, 0, 'unbounded ZIP decompression request')
            self.assertLessEqual(size, 2, 'ZIP read exceeds the advertised size plus one')
            return read(stream, size)

        with patch.object(zipfile.ZipExtFile, 'read', bounded_read):
            self.assertEqual(package.read_archive(archive)['guide.md'][0], b'x')

    def test_normal_zip_comments_and_pax_paths_still_parse(self):
        zipped = self.root / 'normal.zip'
        with zipfile.ZipFile(zipped, 'w', compression=zipfile.ZIP_DEFLATED) as output:
            output.comment = b'archive comment'
            output.writestr('guide.md', b'fixture')
        self.assertEqual(package.read_archive(zipped)['guide.md'][0], b'fixture')
        tarred = self.root / 'normal.tar.gz'
        name = 'docs/' + 'long-path-' * 20 + 'guide.md'
        with tarfile.open(tarred, 'w:gz', format=tarfile.PAX_FORMAT) as output:
            entry = tarfile.TarInfo(name)
            entry.size = 7
            entry.pax_headers = {'comment': 'archive comment'}
            output.addfile(entry, io.BytesIO(b'fixture'))
        self.assertEqual(package.read_archive(tarred)[name][0], b'fixture')

    def test_build_checksum_never_replaces_an_existing_file_or_link(self):
        for target in ('x86_64-unknown-linux-gnu', 'x86_64-pc-windows-msvc'):
            archive = self.fixture(target)
            destination = self.root / ('binaries-' + target)
            package.verify(archive, destination=destination)
            binary_dir = destination / package.archive_stem('0.1.0', target) / 'bin'
            archive.unlink()
            for kind in ('new', 'file', 'symlink', 'hardlink'):
                with self.subTest(target=target, kind=kind):
                    output = self.root / (target + '-' + kind)
                    output.mkdir()
                    checksum = output / (package.archive_name('0.1.0', target) + '.sha256')
                    victim = self.root / (target + '-private-' + kind) if kind in ('symlink', 'hardlink') else checksum
                    if kind != 'new':
                        victim.write_bytes(b'private fixture: preserve these bytes')
                        if kind == 'symlink':
                            checksum.symlink_to(victim)
                        elif kind == 'hardlink':
                            os.link(victim, checksum)
                    def git(*args):
                        return '' if args[0] == 'status' else '1700000000' if args[0] == 'show' else 'a' * 40
                    def command(*args, **kwargs):
                        return SimpleNamespace(stdout='hudson 0.1.0\n' if kwargs.get('text') else b'# fixture completion\n', stderr=b'')
                    rejected = False
                    with patch.object(package, 'git', side_effect=git), \
                            patch.object(package, 'version', return_value='0.1.0'), \
                            patch.object(package, 'dependency_notices', return_value=b'fixture license\n'), \
                            patch.object(package.subprocess, 'run', side_effect=command):
                        try:
                            result = package.build(target, binary_dir, output)
                        except FileExistsError:
                            rejected = True
                    if kind == 'new':
                        self.assertFalse(rejected)
                        self.assertEqual(checksum.read_text(), f'{package.digest(result.read_bytes())}  {result.name}\n')
                    else:
                        self.assertEqual(victim.read_bytes(), b'private fixture: preserve these bytes')
                        self.assertTrue(rejected)
                        self.assertEqual(checksum.is_symlink(), kind == 'symlink')
                    self.assertFalse(list(output.glob('.hudson-checksum-*')))


if __name__ == '__main__':
    unittest.main()
