"""Source and native bottle qualification; no live publication or Homebrew needed."""
import io
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import homebrew
import package


def archive(files, *, prefix='', symlink=False):
    data = io.BytesIO()
    with tarfile.open(fileobj=data, mode='w:gz') as output:
        for name, content in files.items():
            entry = tarfile.TarInfo(prefix + name)
            entry.size, entry.mode = len(content), 0o644
            if symlink:
                entry.type, entry.linkname, entry.size = tarfile.SYMTYPE, '/private/credential', 0
            output.addfile(entry, io.BytesIO(content))
    return data.getvalue()


class HomebrewTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.commit = 'a' * 40
        self.version = '0.1.1'
        self.source_sha = 'b' * 64

    def candidates(self):
        directory = self.root / 'candidates'
        directory.mkdir()
        tags = {'x86_64-unknown-linux-gnu': 'x86_64_linux',
                'aarch64-apple-darwin': 'arm64_sequoia'}
        for target, tag in tags.items():
            folder = directory / ('homebrew-' + target)
            folder.mkdir()
            formula = homebrew.render(self.commit, self.version, self.source_sha)
            (folder / 'hudson.rb').write_text(formula, encoding='utf-8', newline='\n')
            local_name = f'hudson--{self.version}.{tag}.bottle.tar.gz'
            (folder / local_name).write_bytes(target.encode())
            checksum = package.digest(target.encode())
            receipt = {'schema': 1, 'commit': self.commit, 'version': self.version,
                       'source_url': homebrew.source_url(self.commit), 'source_sha256': self.source_sha,
                       'formula_sha256': package.digest(formula.encode()), 'target': target,
                       'bottle_tag': tag,
                       'cellar': 'any' if target.endswith('linux-gnu') else 'any_skip_relocation',
                       'bottle_file': f'hudson-{self.version}.{tag}.bottle.tar.gz',
                       'bottle_sha256': checksum, 'poured_from_bottle': True}
            (folder / 'verified-bottle.json').write_text(json.dumps(receipt))
            metadata = {homebrew.FORMULA: {'formula': {'pkg_version': self.version}, 'bottle': {
                'root_url': f'https://github.com/hudson-frontier/homebrew-tap/releases/download/v{self.version}',
                'rebuild': 0, 'cellar': receipt['cellar'], 'tags': {tag: {
                    'filename': receipt['bottle_file'], 'local_filename': local_name,
                    'sha256': checksum}}}}}
            (folder / (local_name.removesuffix('.tar.gz') + '.json')).write_text(json.dumps(metadata))
        return directory

    def test_source_tree_matches_without_extracting_untrusted_bytes(self):
        files = {'Cargo.toml': b'package\n', 'src/main.rs': b'fn main() {}\n'}
        expected = archive(files)
        downloaded = archive(files, prefix='hudson-source/')
        self.assertEqual(homebrew.verify_source(downloaded, expected), package.digest(downloaded))
        for changed in [{'Cargo.toml': b'package\n'}, dict(files, **{'src/main.rs': b'tampered'})]:
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                homebrew.verify_source(archive(changed, prefix='hudson-source/'), expected)

    def test_source_comparison_ignores_checkout_line_endings(self):
        repository = self.root / 'repository'
        repository.mkdir()

        def git(*args):
            return subprocess.check_output(['git', '-C', str(repository), *args], stderr=subprocess.STDOUT)

        git('init', '--initial-branch=main')
        git('config', 'core.autocrlf', 'true')
        content = b'[package]\nname = "fixture"\n'
        (repository / 'Cargo.toml').write_bytes(content.replace(b'\n', b'\r\n'))
        git('add', 'Cargo.toml')
        git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@hudson.invalid',
            '-c', 'commit.gpgsign=false', 'commit', '-m', 'Source fixture')
        with patch.object(package, 'ROOT', repository):
            expected = homebrew.committed_archive('HEAD')
        downloaded = archive({'Cargo.toml': content}, prefix='hudson-source/')
        self.assertEqual(homebrew.verify_source(downloaded, expected), package.digest(downloaded))

    def test_source_rejects_traversal_symlinks_and_growth(self):
        for name in ['../escape', '/absolute', 'root/../escape', 'root\\escape']:
            with self.subTest(name=name), self.assertRaises(ValueError):
                homebrew.archive_tree(archive({name: b'x'}), prefixed=True)
        with self.assertRaises(ValueError):
            homebrew.archive_tree(archive({'root/link': b''}, symlink=True), prefixed=True)
        with self.assertRaises(ValueError):
            homebrew.archive_tree(b'x' * (homebrew.MAX_SOURCE + 1), prefixed=True)
        with self.assertRaises(ValueError):
            homebrew.archive_tree(archive({'one/file': b'x', 'two/file': b'x'}), prefixed=True)

    def test_source_metadata_is_bounded_before_tar_parsing(self):
        for mode in ('w', 'w:gz'):
            data = io.BytesIO()
            with tarfile.open(fileobj=data, mode=mode, format=tarfile.PAX_FORMAT) as output:
                entry = tarfile.TarInfo('root/guide.md')
                entry.size = 1
                entry.pax_headers = {'comment': 'x' * 2048}
                output.addfile(entry, io.BytesIO(b'x'))
            with self.subTest(mode=mode), patch.object(package, 'MAX_METADATA_BYTES', 1024):
                with self.assertRaises(ValueError):
                    homebrew.archive_tree(data.getvalue(), prefixed=True)

    def test_formula_inputs_cannot_select_moving_refs_or_inject_ruby(self):
        for commit, version, digest in [('main', self.version, self.source_sha),
                                        (self.commit, '0.1.1-rc1', self.source_sha),
                                        (self.commit, '0.1.1";system "bad', self.source_sha),
                                        (self.commit, self.version, 'SHA256SUMS')]:
            with self.subTest(version=version), self.assertRaises(ValueError):
                homebrew.render(commit, version, digest)

    def test_all_native_bottles_compose_without_overwriting_a_bundle(self):
        candidates = self.candidates()
        output = self.root / 'ready'
        result = homebrew.compose(candidates, output, self.commit, self.version, '123')
        self.assertEqual(len(result['bottles']), 2)
        formula = (output / 'tap/Formula/hudson.rb').read_text()
        for name, digest in result['bottles'].items():
            self.assertEqual(package.digest((output / 'bottles' / name).read_bytes()), digest)
            self.assertIn(digest, formula)
        before = (output / 'publication.json').read_bytes()
        with self.assertRaises(ValueError):
            homebrew.compose(candidates, output, self.commit, self.version, '123')
        self.assertEqual((output / 'publication.json').read_bytes(), before)

    def test_incomplete_or_changed_receipts_never_produce_a_publication(self):
        candidates = self.candidates()
        folder = candidates / 'homebrew-x86_64-unknown-linux-gnu'
        receipt_file = folder / 'verified-bottle.json'
        original = json.loads(receipt_file.read_text())
        for field, value in [('commit', 'c' * 40), ('version', '9.9.9'),
                              ('poured_from_bottle', False), ('formula_sha256', 'd' * 64),
                              ('source_sha256', 'e' * 64), ('bottle_tag', 'arm64_sequoia'),
                              ('bottle_file', '../escape'), ('bottle_sha256', 'f' * 64)]:
            receipt_file.write_text(json.dumps({**original, field: value}))
            with self.subTest(field=field), self.assertRaises(ValueError):
                homebrew.compose(candidates, self.root / 'ready', self.commit, self.version, '123')
            self.assertFalse((self.root / 'ready').exists())
        receipt_file.write_text(json.dumps(original))
        (folder / f'hudson--{self.version}.x86_64_linux.bottle.tar.gz').write_bytes(b'changed')
        with self.assertRaises(ValueError):
            homebrew.compose(candidates, self.root / 'ready', self.commit, self.version, '123')
        self.assertFalse((self.root / 'ready').exists())

    def test_composed_bottle_checksums_follow_homebrew_alignment(self):
        candidates = self.candidates()
        output = self.root / 'ready'
        homebrew.compose(candidates, output, self.commit, self.version, '123')
        formula_path = output / 'tap/Formula/hudson.rb'
        if os.name == 'posix':
            self.assertEqual(formula_path.stat().st_mode & 0o777, 0o644)
            self.assertEqual(output.stat().st_mode & 0o777, 0o700)
        formula = formula_path.read_text()
        lines = [line for line in formula.splitlines() if line.strip().startswith('sha256 cellar:')]
        self.assertEqual(len(lines), 2)
        for expression in [r'\b(?:arm64_sequoia|x86_64_linux):', r'"[0-9a-f]{64}"']:
            self.assertEqual(len({re.search(expression, line).start() for line in lines}), 1)


if __name__ == '__main__':
    unittest.main()
