"""Build and verify native CLI archives; publishing is handled separately."""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import gzip
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    'x86_64-unknown-linux-gnu': ('linux', 'x86_64'),
    'aarch64-unknown-linux-gnu': ('linux', 'aarch64'),
    'x86_64-apple-darwin': ('macos', 'x86_64'),
    'aarch64-apple-darwin': ('macos', 'aarch64'),
    'x86_64-pc-windows-msvc': ('windows', 'x86_64'),
}
BINARIES = ('hudson', 'hudson-cli', 'hudsonbox')
SHELL_FILES = {'bash': 'hudson.bash', 'zsh': '_hudson', 'fish': 'hudson.fish',
               'powershell': 'hudson.ps1', 'elvish': 'hudson.elv'}
REQUIRED_DOCS = ('install.md', 'agent.md', 'sandbox.md', 'automation.md',
                 'troubleshooting.md', 'reference.md', 'releasing.md', 'homebrew.md')
MAX_ARCHIVE_BYTES = 512 * 1024 * 1024
MAX_EXPANDED_BYTES = 1024 * 1024 * 1024
MAX_METADATA_BYTES = 1024 * 1024
MAX_MANIFEST_BYTES = 1024 * 1024
MAX_ARCHIVE_MEMBERS = 256


class ArchiveReader:
    """Bound metadata reads and TAR positions before container parsers use them."""

    def __init__(self, source, max_position=None):
        self.source = source
        self.remaining = MAX_METADATA_BYTES
        self.max_position = max_position

    def read(self, size=-1):
        limits = []
        if self.remaining is not None:
            limits.append(self.remaining)
        if self.max_position is not None:
            limits.append(self.max_position - self.tell())
        limit = min(limits) if limits else None
        if limit is not None:
            if size < 0:
                size = limit + 1
            elif size > limit:
                raise ValueError('Archive metadata or expansion budget exceeded')
        data = self.source.read(size)
        if limit is not None and len(data) > limit:
            raise ValueError('Archive metadata or expansion budget exceeded')
        if self.remaining is not None:
            self.remaining -= len(data)
        return data

    def tell(self):
        return self.source.tell()

    def seek(self, offset, whence=0):
        if self.max_position is None:
            return self.source.seek(offset, whence)
        if whence not in (0, 1):
            raise ValueError('Unsupported archive seek')
        position = offset + (self.tell() if whence == 1 else 0)
        if not 0 <= position <= self.max_position:
            raise ValueError('Archive expansion budget exceeded')
        return self.source.seek(position)

    def seekable(self):
        return self.source.seekable()

    @contextmanager
    def payload(self):
        remaining = self.remaining
        self.remaining = None
        try:
            yield
        finally:
            self.remaining = remaining


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def publish_checksum(path: Path, text: str) -> None:
    """Publish complete checksum bytes without replacing any existing entry."""
    descriptor, name = tempfile.mkstemp(prefix='.hudson-checksum-', dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, 'w', encoding='utf-8', newline='\n') as output:
            output.write(text)
            output.flush()
            os.fsync(output.fileno())
        if os.name == 'nt':
            # Windows rename refuses an existing destination; POSIX rename replaces it.
            os.rename(temporary, path)
        else:
            os.link(temporary, path, follow_symlinks=False)
    finally:
        temporary.unlink(missing_ok=True)


def version() -> str:
    value = tomllib.loads((ROOT / 'Cargo.toml').read_text())['package']['version']
    if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?', value):
        raise ValueError('Cargo package has an invalid release version')
    packages = tomllib.loads((ROOT / 'Cargo.lock').read_text())['package']
    found = [p for p in packages if p['name'] == 'hudson-cli' and 'source' not in p]
    if len(found) != 1 or found[0]['version'] != value:
        raise ValueError('Cargo.toml and Cargo.lock versions differ')
    return value


def archive_stem(release_version: str, target: str) -> str:
    system, architecture = TARGETS[target]
    return f'hudson-cli_{release_version}_{system}_{architecture}'


def archive_name(release_version: str, target: str) -> str:
    suffix = '.zip' if TARGETS[target][0] == 'windows' else '.tar.gz'
    return archive_stem(release_version, target) + suffix


def binary_header(data: bytes, target: str) -> None:
    system, architecture = TARGETS[target]
    if system == 'linux':
        valid = len(data) >= 64 and data[:6] == b'\x7fELF\x02\x01'
        valid = valid and struct.unpack_from('<H', data, 18)[0] == (
            62 if architecture == 'x86_64' else 183)
    elif system == 'macos':
        valid = len(data) >= 32 and data[:4] == b'\xcf\xfa\xed\xfe'
        valid = valid and struct.unpack_from('<I', data, 4)[0] == (
            0x01000007 if architecture == 'x86_64' else 0x0100000C)
    else:
        valid = len(data) >= 64 and data[:2] == b'MZ'
        offset = struct.unpack_from('<I', data, 60)[0] if valid else len(data)
        valid = valid and offset + 24 <= len(data) and data[offset:offset+4] == b'PE\0\0'
        valid = valid and struct.unpack_from('<H', data, offset + 4)[0] == 0x8664
    if not valid:
        raise ValueError(f'Binary does not match {target}')


def safe_name(name: str) -> PurePosixPath:
    path = PurePosixPath(name)
    if (not name or not name.isprintable() or any(char in name for char in '\\:<>"|?*')
            or path.is_absolute()):
        raise ValueError('Unsafe archive path')
    if any(part in ('', '.', '..') for part in name.split('/')):
        raise ValueError('Unsafe archive path')
    reserved = {'CON', 'PRN', 'AUX', 'NUL', 'CONIN$', 'CONOUT$'} | {
        f'{prefix}{number}' for prefix in ('COM', 'LPT') for number in '123456789\u00b9\u00b2\u00b3'}
    if any(part.rstrip(' .') != part or part.split('.')[0].rstrip(' ').upper() in reserved
           for part in path.parts):
        raise ValueError('Unsafe portable archive path')
    return path


def read_archive(archive: Path) -> dict[str, tuple[bytes, int]]:
    if archive.stat().st_size > MAX_ARCHIVE_BYTES:
        raise ValueError('Archive is too large')
    entries: dict[str, tuple[bytes, int]] = {}
    names: set[str] = set()
    total = 0

    def add(name: str, data: bytes, mode: int) -> None:
        nonlocal total
        safe_name(name)
        if name.casefold() in names:
            raise ValueError('Duplicate archive path')
        names.add(name.casefold())
        total += len(data)
        if total > MAX_EXPANDED_BYTES:
            raise ValueError('Expanded archive is too large')
        entries[name] = (data, mode)

    if archive.name.endswith('.zip'):
        with archive.open('rb') as raw:
            reader = ArchiveReader(raw)
            with zipfile.ZipFile(reader) as source:
                if len(source.infolist()) > MAX_ARCHIVE_MEMBERS:
                    raise ValueError('Archive has too many members')
                for entry in source.infolist():
                    safe_name(entry.orig_filename.rstrip('/'))
                    if entry.is_dir():
                        safe_name(entry.filename.rstrip('/'))
                        continue
                    mode = entry.external_attr >> 16
                    limit = MAX_EXPANDED_BYTES - total
                    if entry.filename.endswith('/manifest.json'):
                        limit = min(limit, MAX_MANIFEST_BYTES)
                    if (stat.S_ISLNK(mode) or entry.file_size > limit
                            or entry.compress_size > MAX_ARCHIVE_BYTES
                            or entry.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED)):
                        raise ValueError('Unsupported archive member')
                    with source.open(entry) as member:
                        with reader.payload():
                            data = member.read(entry.file_size + 1)
                    if len(data) != entry.file_size:
                        raise ValueError('Archive member size does not match')
                    add(entry.filename, data, stat.S_IMODE(mode))
    elif archive.name.endswith('.tar.gz'):
        try:
            with gzip.open(archive, 'rb') as raw:
                reader = ArchiveReader(raw, MAX_EXPANDED_BYTES + MAX_METADATA_BYTES
                                       + MAX_ARCHIVE_MEMBERS * tarfile.BLOCKSIZE)
                with tarfile.open(fileobj=reader, mode='r:') as source:
                    for number, entry in enumerate(source, 1):
                        if number > MAX_ARCHIVE_MEMBERS:
                            raise ValueError('Archive has too many members')
                        if entry.size < 0:
                            raise ValueError('Unsupported archive member')
                        if entry.isdir():
                            if entry.size:
                                raise ValueError('Unsupported directory body')
                            safe_name(entry.name.rstrip('/'))
                            continue
                        limit = MAX_EXPANDED_BYTES - total
                        if entry.name.endswith('/manifest.json'):
                            limit = min(limit, MAX_MANIFEST_BYTES)
                        if not entry.isfile() or entry.size > limit:
                            raise ValueError('Unsupported archive member')
                        stream = source.extractfile(entry)
                        if stream is None:
                            raise ValueError('Missing archive member')
                        with reader.payload(), stream:
                            add(entry.name, stream.read(entry.size + 1), entry.mode)
        except RecursionError as error:
            raise ValueError('Archive metadata nesting is too deep') from error
    else:
        raise ValueError('Unsupported archive format')
    return entries


def verify(archive: Path, expected_commit: str | None = None,
           destination: Path | None = None) -> dict:
    entries = read_archive(archive)
    manifest_paths = [name for name in entries if name.endswith('/manifest.json')]
    if len(manifest_paths) != 1:
        raise ValueError('Archive needs exactly one manifest')
    manifest_bytes = entries[manifest_paths[0]][0]
    if entries[manifest_paths[0]][1] != 0o644:
        raise ValueError('Manifest has an unexpected mode')
    if len(manifest_bytes) > MAX_MANIFEST_BYTES:
        raise ValueError('Manifest is too large')
    manifest = json.loads(manifest_bytes)
    if manifest.get('schema') != 1 or manifest.get('target') not in TARGETS:
        raise ValueError('Invalid manifest schema or target')
    if not re.fullmatch(r'\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?', manifest.get('version', '')):
        raise ValueError('Invalid manifest version')
    commit = manifest.get('commit', '')
    if not re.fullmatch(r'[0-9a-f]{40}', commit) or expected_commit and commit != expected_commit:
        raise ValueError('Archive source revision does not match')
    target = manifest['target']
    prefix = archive_stem(manifest['version'], target) + '/'
    if archive.name != archive_name(manifest['version'], target):
        raise ValueError('Archive filename does not match its manifest')
    files = manifest.get('files', {})
    if not isinstance(files, dict):
        raise ValueError('Invalid manifest file list')
    expected = {prefix + name for name in files} | {prefix + 'manifest.json'}
    if set(entries) != expected:
        raise ValueError('Archive contents do not match the manifest')
    executable_suffix = '.exe' if TARGETS[target][0] == 'windows' else ''
    required = {f'bin/{name}{executable_suffix}' for name in BINARIES}
    required |= {f'completions/{name}' for name in SHELL_FILES.values()}
    required |= {'README.md', 'LICENSE', 'NOTICE', 'CHANGELOG.md', 'SECURITY.md', 'THIRD_PARTY_NOTICES.txt'}
    required |= {f'docs/{name}' for name in REQUIRED_DOCS}
    if not required.issubset(files):
        raise ValueError('Archive is missing a required release file')
    if {name for name in files if name.startswith('bin/')} != {
            f'bin/{name}{executable_suffix}' for name in BINARIES}:
        raise ValueError('Archive contains unexpected executables')
    for name in files:
        path = safe_name(name)
        if name not in required and not (path.parts[0] == 'docs' and path.suffix == '.md'):
            raise ValueError('Archive contains an unexpected release file')
        if any(str(parent) in files for parent in path.parents):
            raise ValueError('Archive file collides with a directory')
    for name, record in files.items():
        safe_name(name)
        data, mode = entries[prefix + name]
        if record != {'sha256': digest(data), 'size': len(data), 'mode': mode}:
            raise ValueError(f'Archive integrity check failed: {name}')
        if name.startswith('bin/'):
            if mode != 0o755:
                raise ValueError('Release binary is not executable')
            binary_header(data, target)
        elif mode != 0o644:
            raise ValueError('Release data file has an unexpected mode')
    if destination is not None:
        if destination.exists() and any(destination.iterdir()):
            raise ValueError('Installation test directory must be empty')
        destination.mkdir(parents=True, exist_ok=True)
        for name, (data, mode) in entries.items():
            path = destination.joinpath(*safe_name(name).parts)
            path.parent.mkdir(parents=True, exist_ok=True)
            with path.open('xb') as output:
                output.write(data)
            path.chmod(mode)
    return manifest


def git(*args: str) -> str:
    return subprocess.check_output(['git', '-c', f'safe.directory={ROOT.as_posix()}',
                                    '-C', str(ROOT), *args], text=True).strip()


def dependency_notices(target: str) -> bytes:
    metadata = json.loads(subprocess.check_output(
        ['cargo', 'metadata', '--locked', '--format-version', '1', '--filter-platform', target],
        cwd=ROOT, text=True))
    packages = {item['id']: item for item in metadata['packages']}
    nodes = {item['id']: item for item in metadata['resolve']['nodes']}
    root = metadata['resolve']['root']
    pending, included = [root], set()
    while pending:
        identifier = pending.pop()
        if identifier in included:
            continue
        included.add(identifier)
        for dependency in nodes[identifier]['deps']:
            if any(kind['kind'] != 'dev' for kind in dependency['dep_kinds']):
                pending.append(dependency['pkg'])
    sections = ['Dependency license notices for the packaged native client.\n']
    for item in sorted((packages[name] for name in included if name != root),
                       key=lambda value: (value['name'], value['version'])):
        directory = Path(item['manifest_path']).parent
        license_files = []
        if item.get('license_file'):
            path = Path(item['license_file'])
            license_files.append(path if path.is_absolute() else directory / path)
        for base in [directory, *list(directory.parents)[:2]]:
            found = [path for path in base.iterdir() if path.is_file() and
                     path.name.upper().startswith(('LICENSE', 'COPYING', 'NOTICE'))]
            if found:
                license_files.extend(found)
                break
        license_files = sorted(set(license_files))
        if not license_files:
            raise ValueError(f'Missing dependency license text: {item["name"]} {item["version"]}')
        sections.append(f'\n{"=" * 72}\n{item["name"]} {item["version"]}\n'
                        f'License expression: {item.get("license") or "see license text"}\n')
        for path in license_files:
            if path.stat().st_size > 1024 * 1024:
                raise ValueError('Unexpected dependency license size')
            sections.append(f'\n--- {path.name} ---\n{path.read_text(encoding="utf-8")}\n')
    return '\n'.join(sections).encode('utf-8')


def build(target: str, binary_dir: Path, output: Path, allow_dirty: bool = False) -> Path:
    release_version = version()
    commit = git('rev-parse', 'HEAD')
    dirty = bool(git('status', '--porcelain', '--untracked-files=normal'))
    if dirty and not allow_dirty:
        raise ValueError('Commit release inputs before packaging')
    epoch = int(git('show', '-s', '--format=%ct', 'HEAD'))
    files: dict[str, tuple[bytes, int]] = {}
    suffix = '.exe' if TARGETS[target][0] == 'windows' else ''
    environment = dict(os.environ)
    environment.pop('HUDSONBOX_CONFIG', None)
    for name in BINARIES:
        binary = binary_dir / (name + suffix)
        if binary.is_symlink():
            raise ValueError('Release binary must be a regular file')
        data = binary.read_bytes()
        binary_header(data, target)
        result = subprocess.run([str(binary), '--version'], check=True, capture_output=True,
                                text=True, timeout=15, env=environment)
        if result.stdout.strip().split()[-1] != release_version:
            raise ValueError('Binary version does not match Cargo metadata')
        files[f'bin/{name}{suffix}'] = (data, 0o755)
    for shell, name in SHELL_FILES.items():
        result = subprocess.run([str(binary_dir / ('hudson' + suffix)), 'completion', shell],
                                check=True, capture_output=True, timeout=15, env=environment)
        if not result.stdout or result.stderr:
            raise ValueError('Completion generation failed')
        files[f'completions/{name}'] = (result.stdout, 0o644)
    for name in ('README.md', 'LICENSE', 'NOTICE', 'CHANGELOG.md', 'SECURITY.md'):
        files[name] = ((ROOT / name).read_text(encoding='utf-8').encode(), 0o644)
    files['THIRD_PARTY_NOTICES.txt'] = (dependency_notices(target), 0o644)
    for path in sorted((ROOT / 'docs').rglob('*.md')):
        if path.is_symlink():
            raise ValueError('Release documentation must be a regular file')
        files[path.relative_to(ROOT).as_posix()] = (path.read_text(encoding='utf-8').encode(), 0o644)
    manifest = {'schema': 1, 'version': release_version, 'commit': commit, 'target': target,
                'dirty': dirty, 'files': {name: {'sha256': digest(data), 'size': len(data), 'mode': mode}
                                        for name, (data, mode) in sorted(files.items())}}
    files['manifest.json'] = (json.dumps(manifest, indent=2, sort_keys=True).encode() + b'\n', 0o644)
    output.mkdir(parents=True, exist_ok=True)
    archive = output / archive_name(release_version, target)
    prefix = archive_stem(release_version, target)
    if archive.suffix == '.zip':
        stamp = time.gmtime(max(epoch, 315532800))[:6]
        with zipfile.ZipFile(archive, 'x', compression=zipfile.ZIP_DEFLATED, compresslevel=9) as bundle:
            for name, (data, mode) in sorted(files.items()):
                entry = zipfile.ZipInfo(f'{prefix}/{name}', stamp)
                entry.create_system = 3
                entry.external_attr = (stat.S_IFREG | mode) << 16
                entry.compress_type = zipfile.ZIP_DEFLATED
                bundle.writestr(entry, data)
    else:
        with archive.open('xb') as raw:
            with gzip.GzipFile(filename='', fileobj=raw, mode='wb', mtime=epoch) as compressed:
                with tarfile.open(fileobj=compressed, mode='w', format=tarfile.PAX_FORMAT) as bundle:
                    for name, (data, mode) in sorted(files.items()):
                        entry = tarfile.TarInfo(f'{prefix}/{name}')
                        entry.size, entry.mode, entry.mtime = len(data), mode, epoch
                        bundle.addfile(entry, io.BytesIO(data))
    verify(archive, commit)
    publish_checksum(archive.with_name(archive.name + '.sha256'),
                     f'{digest(archive.read_bytes())}  {archive.name}\n')
    return archive


def install_test(archive: Path) -> None:
    with tempfile.TemporaryDirectory(prefix='hudson fresh install ') as temporary:
        root = Path(temporary)
        manifest = verify(archive, destination=root)
        environment = dict(os.environ, HUDSON_TEST_BIN_DIR=str(
            root / archive_stem(manifest['version'], manifest['target']) / 'bin'))
        subprocess.run(['cargo', 'test', '--release', '--locked', '--all-targets',
                        '--target', manifest['target']], cwd=ROOT, env=environment, check=True)
        if os.name == 'posix':
            subprocess.run([sys.executable, 'scripts/check_session.py', '--binary',
                            str(Path(environment['HUDSON_TEST_BIN_DIR']) / 'hudson')],
                           cwd=ROOT, env=environment, check=True)
        if manifest['target'].endswith('-unknown-linux-gnu'):
            subprocess.run([sys.executable, 'scripts/check_response_memory.py', '--binary-dir',
                            environment['HUDSON_TEST_BIN_DIR']],
                           cwd=ROOT, env=environment, check=True)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    subcommands = parser.add_subparsers(dest='action', required=True)
    build_parser = subcommands.add_parser('build')
    build_parser.add_argument('--target', choices=TARGETS, required=True)
    build_parser.add_argument('--binary-dir', type=Path, required=True)
    build_parser.add_argument('--output', type=Path, default=ROOT / 'dist')
    build_parser.add_argument('--allow-dirty', action='store_true')
    check_parser = subcommands.add_parser('check')
    check_parser.add_argument('--archive', type=Path, required=True)
    check_parser.add_argument('--commit')
    check_parser.add_argument('--destination', type=Path)
    check_parser.add_argument('--install-test', action='store_true')
    args = parser.parse_args()
    if args.action == 'build':
        print(build(args.target, args.binary_dir.resolve(), args.output, args.allow_dirty))
    else:
        manifest = verify(args.archive, args.commit, args.destination)
        if args.install_test:
            install_test(args.archive)
        print(json.dumps({key: manifest[key] for key in ('version', 'commit', 'target')}))


if __name__ == '__main__':
    main()
