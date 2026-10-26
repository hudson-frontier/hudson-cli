"""Prepare a pinned Homebrew source formula; never publish a package or tag."""
from __future__ import annotations

import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import shutil
import subprocess
import tarfile
import urllib.request

import package

TAP = 'hudson-frontier/tap'
FORMULA = TAP + '/hudson'
REPOSITORY = 'hudson-frontier/hudson-cli'
TEMPLATE = package.ROOT / 'packaging/homebrew/hudson.rb.in'
MAX_SOURCE = 32 * 1024 * 1024
MAX_FILES = 4096
SUPPORTED_TARGETS = {
    'x86_64-unknown-linux-gnu': 'x86_64_linux',
    'aarch64-apple-darwin': 'arm64_sequoia',
}


def identifiers(commit: str, version: str, checksum: str) -> None:
    if not re.fullmatch(r'[0-9a-f]{40}', commit):
        raise ValueError('Use an immutable source commit')
    if not re.fullmatch(r'\d+\.\d+\.\d+', version):
        raise ValueError('Homebrew packages require a final release version')
    if not re.fullmatch(r'[0-9a-f]{64}', checksum):
        raise ValueError('Use a complete source SHA-256 digest')


def source_url(commit: str) -> str:
    identifiers(commit, '0.0.0', '0' * 64)
    return f'https://github.com/{REPOSITORY}/archive/{commit}.tar.gz'


def archive_tree(data: bytes, *, prefixed: bool) -> dict[str, tuple[str, bool]]:
    """Compare source trees without extracting downloaded files onto the host."""
    if len(data) > MAX_SOURCE:
        raise ValueError('Source archive exceeds its byte limit')
    files = {}
    root = None
    total = 0
    count = 0
    with tarfile.open(fileobj=io.BytesIO(data), mode='r:*') as archive:
        for member in archive:
            count += 1
            if count > MAX_FILES:
                raise ValueError('Source archive exceeds its file limit')
            path = package.safe_name(member.name.rstrip('/'))
            if prefixed:
                if root is None:
                    root = path.parts[0]
                if path.parts[0] != root:
                    raise ValueError('Source archive has multiple roots')
                if len(path.parts) == 1:
                    if not member.isdir():
                        raise ValueError('Invalid source archive root')
                    continue
                name = '/'.join(path.parts[1:])
            else:
                name = str(path)
            if member.isdir():
                continue
            if not member.isfile() or member.size > MAX_SOURCE - total or name in files:
                raise ValueError('Invalid source archive member')
            stream = archive.extractfile(member)
            if stream is None:
                raise ValueError('Missing source archive member')
            content = stream.read(MAX_SOURCE - total + 1)
            total += len(content)
            if len(content) != member.size or total > MAX_SOURCE:
                raise ValueError('Source archive content exceeds its byte limit')
            files[name] = (hashlib.sha256(content).hexdigest(), bool(member.mode & 0o111))
    if not files:
        raise ValueError('Source archive is empty')
    return files


def verify_source(data: bytes, expected: bytes) -> str:
    if archive_tree(data, prefixed=True) != archive_tree(expected, prefixed=False):
        raise ValueError('Downloaded source does not match the selected commit')
    return hashlib.sha256(data).hexdigest()


def committed_archive(commit: str) -> bytes:
    # Match GitHub's committed bytes even when the caller checks out CRLF files.
    return subprocess.check_output(['git', '-c', 'core.autocrlf=false', '-c', 'core.eol=lf',
                                    'archive', '--format=tar', commit], cwd=package.ROOT)


def render(commit: str, version: str, checksum: str) -> str:
    identifiers(commit, version, checksum)
    text = TEMPLATE.read_text(encoding='utf-8')
    for token, value in {'COMMIT': commit, 'VERSION': version, 'SHA256': checksum}.items():
        marker = '@' + token + '@'
        if text.count(marker) != 1:
            raise ValueError('Unexpected Homebrew template layout')
        text = text.replace(marker, value)
    if re.search(r'@[A-Z_]+@', text):
        raise ValueError('Unresolved Homebrew template value')
    return text


def prepare(destination: Path) -> dict:
    if package.git('status', '--porcelain', '--untracked-files=no'):
        raise ValueError('Commit source changes before preparing a formula')
    commit = package.git('rev-parse', 'HEAD')
    version = package.version()
    identifiers(commit, version, '0' * 64)
    if destination.exists() or destination.is_symlink():
        raise ValueError('Use a new Homebrew output directory')
    destination.mkdir(parents=True, mode=0o700)
    expected = committed_archive(commit)
    url = source_url(commit)
    with urllib.request.urlopen(url, timeout=60) as response:
        if not response.geturl().startswith(('https://github.com/', 'https://codeload.github.com/')):
            raise ValueError('Unexpected source download endpoint')
        data = response.read(MAX_SOURCE + 1)
    checksum = verify_source(data, expected)
    text = render(commit, version, checksum)
    receipt = {'schema': 1, 'version': version, 'commit': commit,
               'source_url': url, 'source_sha256': checksum,
               'formula_sha256': package.digest(text.encode())}
    package.publish_checksum(destination / 'hudson.rb', text)
    package.publish_checksum(destination / 'source.json', json.dumps(receipt, indent=2, sort_keys=True) + '\n')
    return receipt


def verify_bottle(directory: Path, source: dict, target: str, tag: str, spec: dict, cellar: str) -> Path:
    if SUPPORTED_TARGETS.get(target) != tag:
        raise ValueError('Bottle tag does not match the native target')
    if cellar not in {'any', 'any_skip_relocation', '/opt/homebrew/Cellar', '/usr/local/Cellar',
                      '/home/linuxbrew/.linuxbrew/Cellar'}:
        raise ValueError('Unexpected bottle Cellar')
    for key in ['filename', 'local_filename']:
        name = spec[key]
        if (not isinstance(name, str) or '/' in name or '\\' in name
                or not re.fullmatch(r'hudson--?' + re.escape(source['version']) + r'\.'
                                    + re.escape(tag) + r'\.bottle\.tar\.gz', name)):
            raise ValueError('Unexpected native bottle filename')
    path = directory / spec['local_filename']
    if path.is_symlink() or not path.is_file() or path.stat().st_size > package.MAX_ARCHIVE_BYTES:
        raise ValueError('Bottle must be a bounded regular file')
    if package.digest(path.read_bytes()) != spec['sha256']:
        raise ValueError('Bottle checksum differs from its manifest')
    return path


def compose(directory: Path, output: Path, commit: str, version: str, ci_run: str) -> dict:
    """Compose only matching, installed native candidates into a reviewable tap."""
    identifiers(commit, version, '0' * 64)
    if output.exists() or output.is_symlink():
        raise ValueError('Use a new Homebrew publication directory')
    candidates = {}
    source = None
    for target in sorted(SUPPORTED_TARGETS):
        candidate = directory / ('homebrew-' + target)
        receipt_file = candidate / 'verified-bottle.json'
        if receipt_file.is_symlink() or receipt_file.stat().st_size > 65536:
            raise ValueError('Invalid native bottle receipt')
        receipt = json.loads(receipt_file.read_text())
        if (receipt['schema'] != 1 or receipt['commit'] != commit or receipt['version'] != version
                or receipt['target'] != target or receipt['poured_from_bottle'] is not True
                or receipt['source_url'] != source_url(commit)):
            raise ValueError('Native bottle receipt does not match qualified source')
        text = render(commit, version, receipt['source_sha256'])
        if (receipt['formula_sha256'] != package.digest(text.encode())
                or (candidate / 'hudson.rb').read_text() != text):
            raise ValueError('Native bottle formula differs from the selected recipe')
        selected = {key: receipt[key] for key in ['commit', 'version', 'source_url', 'source_sha256', 'formula_sha256']}
        if source is not None and source != selected:
            raise ValueError('Native bottles used different source bytes')
        source = selected
        metadata_files = list(candidate.glob('*.bottle.json'))
        if len(metadata_files) != 1 or metadata_files[0].is_symlink():
            raise ValueError('Expected exactly one native bottle manifest')
        metadata = json.loads(metadata_files[0].read_text())[FORMULA]
        bottle = metadata['bottle']
        tag = receipt['bottle_tag']
        expected_url = f'https://github.com/hudson-frontier/homebrew-tap/releases/download/v{version}'
        if (metadata['formula']['pkg_version'] != version or bottle['root_url'] != expected_url
                or bottle['rebuild'] != 0 or set(bottle['tags']) != {tag}
                or bottle['cellar'] != receipt['cellar']):
            raise ValueError('Native bottle manifest does not match its receipt')
        spec = bottle['tags'][tag]
        if (spec['filename'] != receipt['bottle_file']
                or spec['sha256'] != receipt['bottle_sha256'] or tag in candidates):
            raise ValueError('Native bottle identity is inconsistent')
        path = verify_bottle(candidate, receipt, target, tag, spec, bottle['cellar'])
        candidates[tag] = (path, receipt)
    block = ['  bottle do', f'    root_url "https://github.com/hudson-frontier/homebrew-tap/releases/download/v{version}"']
    checksums = []
    for tag, (_, receipt) in sorted(candidates.items()):
        cellar = receipt['cellar']
        value = ':' + cellar if cellar in {'any', 'any_skip_relocation'} else '"' + cellar + '"'
        checksums.append((f'    sha256 cellar: {value},', tag + ':', receipt['bottle_sha256']))
    cellar_width = max(len(prefix) for prefix, _, _ in checksums)
    tag_width = max(len(tag) for _, tag, _ in checksums)
    for prefix, tag, checksum in checksums:
        block.append(f'{prefix.ljust(cellar_width)} {tag.ljust(tag_width)} "{checksum}"')
    block.extend(['  end', ''])
    formula = render(commit, version, source['source_sha256'])
    formula = formula.replace('  depends_on "rust"', '\n'.join(block) + '\n  depends_on "rust"', 1)
    output.mkdir(parents=True, mode=0o700)
    tap = output / 'tap'
    shutil.copytree(package.ROOT / 'packaging/homebrew/tap', tap)
    (tap / 'Formula').mkdir()
    package.publish_checksum(tap / 'Formula/hudson.rb', formula)
    shutil.copyfile(package.ROOT / 'LICENSE', tap / 'LICENSE')
    bottles = output / 'bottles'
    bottles.mkdir()
    records = {}
    for path, receipt in candidates.values():
        destination = bottles / receipt['bottle_file']
        with destination.open('xb') as stream:
            stream.write(path.read_bytes())
        records[destination.name] = receipt['bottle_sha256']
    manifest = {'schema': 1, 'ci_run': ci_run, 'source': source,
                'formula_sha256': package.digest(formula.encode()), 'bottles': records}
    package.publish_checksum(output / 'publication.json', json.dumps(manifest, indent=2, sort_keys=True) + '\n')
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='action', required=True)
    prepare_parser = commands.add_parser('prepare')
    prepare_parser.add_argument('--output', type=Path, required=True)
    compose_parser = commands.add_parser('compose')
    compose_parser.add_argument('--directory', type=Path, required=True)
    compose_parser.add_argument('--output', type=Path, required=True)
    compose_parser.add_argument('--ci-run', required=True)
    args = parser.parse_args()
    if args.action == 'prepare':
        receipt = prepare(args.output.resolve())
    else:
        import release
        source = release.verify_run(args.ci_run)
        receipt = compose(args.directory.resolve(), args.output.resolve(), source['commit'],
                          source['version'], args.ci_run)
    print(json.dumps(receipt, sort_keys=True))


if __name__ == '__main__':
    main()
