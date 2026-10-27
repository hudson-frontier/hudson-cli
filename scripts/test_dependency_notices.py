"""Verify offline, version-bound license supplements without relaxing failures."""
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import package


class DependencyNoticesTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.entries = json.loads((package.ROOT / 'packaging/licenses/manifest.json').read_text())['packages']

    def crate(self, entry):
        directory = self.root / entry['name']
        directory.mkdir()
        (directory / '.cargo_vcs_info.json').write_text(json.dumps({
            'git': {'sha1': entry['vcs']}, 'path_in_vcs': entry['path_in_vcs']}))
        item = {key: entry[key] for key in ('name', 'version', 'repository', 'license')}
        item.update(source='registry+https://github.com/rust-lang/crates.io-index',
                    manifest_path=str(directory / 'Cargo.toml'))
        return item, directory

    def test_all_reviewed_notices_are_bundled_with_exact_copyright_and_context(self):
        (self.root / 'LICENSE').write_text('Unrelated registry parent notice must not be used')
        packages = [{'id': 'root'}]
        nodes = [{'id': 'root', 'deps': []}]
        for entry in self.entries:
            item, _ = self.crate(entry)
            item['id'] = entry['name']
            packages.append(item)
            nodes.append({'id': item['id'], 'deps': []})
            nodes[0]['deps'].append({'pkg': item['id'], 'dep_kinds': [{'kind': None}]})
        metadata = {'packages': packages, 'resolve': {'root': 'root', 'nodes': nodes}}
        with patch.object(package.subprocess, 'check_output', return_value=json.dumps(metadata)):
            text = package.dependency_notices('x86_64-unknown-linux-gnu').decode()
        for expected in ['Copyright (c) 2020-2026 Dmitry Dygalo', 'Copyright (c) 2021 Nugine',
                         'All other crates are trio-licensed', 'Apple SDKs']:
            self.assertIn(expected, text)
        for entry in self.entries:
            self.assertIn(entry['name'] + ' ' + entry['version'], text)
            self.assertIn(entry['vcs'], text)

    def test_changed_crate_identity_or_unknown_missing_text_fails(self):
        item, directory = self.crate(self.entries[0])
        for field, value in [('name', 'unknown-crate'), ('version', '99.0.0'),
                             ('repository', 'https://example.invalid/other'),
                             ('license', 'Apache-2.0'), ('source', 'git+https://example.invalid')]:
            with self.subTest(field=field), self.assertRaises(ValueError):
                package.supplemental_license_notices(dict(item, **{field: value}), directory)

    def test_changed_vcs_or_dirty_package_fails(self):
        entry = self.entries[0]
        item, directory = self.crate(entry)
        for vcs in [{'git': {'sha1': 'a' * 40}, 'path_in_vcs': entry['path_in_vcs']},
                    {'git': {'sha1': entry['vcs'], 'dirty': True}, 'path_in_vcs': entry['path_in_vcs']},
                    {'git': {'sha1': entry['vcs']}, 'path_in_vcs': 'other'}]:
            (directory / '.cargo_vcs_info.json').write_text(json.dumps(vcs))
            with self.assertRaises(ValueError):
                package.supplemental_license_notices(item, directory)

    def test_changed_vendored_bytes_fail(self):
        entry = self.entries[0]
        item, directory = self.crate(entry)
        alternate = self.root / 'alternate'
        shutil.copytree(package.ROOT / 'packaging/licenses', alternate / 'packaging/licenses')
        (alternate / 'packaging/licenses' / entry['files'][0]['path']).write_text('tampered')
        with patch.object(package, 'ROOT', alternate), self.assertRaises(ValueError):
            package.supplemental_license_notices(item, directory)
