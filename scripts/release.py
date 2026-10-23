"""Promote only tested main-branch archives through a verified draft release."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import urllib.error
import urllib.request

import package

REPOSITORY = 'hudson-frontier/hudson-cli'


def api(path: str, method: str = 'GET', body: dict | None = None, missing=False):
    token = os.environ.get('GH_TOKEN')
    if not token:
        raise ValueError('GH_TOKEN is required for release administration')
    request = urllib.request.Request(
        f'https://api.github.com/repos/{REPOSITORY}/{path}',
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers={'Authorization': f'Bearer {token}', 'Accept': 'application/vnd.github+json',
                 'X-GitHub-Api-Version': '2022-11-28', 'Content-Type': 'application/json'},
    )
    try:
        with urllib.request.urlopen(request, timeout=30) as response:
            return json.load(response)
    except urllib.error.HTTPError as error:
        if missing and error.code == 404:
            return None
        raise ValueError(f'GitHub release administration failed with HTTP {error.code}') from None


def workflow_output(name: str, value: str) -> None:
    if '\n' in value or '\r' in value:
        raise ValueError('Invalid workflow output')
    if path := os.environ.get('GITHUB_OUTPUT'):
        with open(path, 'a', encoding='utf-8', newline='\n') as output:
            output.write(f'{name}={value}\n')


def verify_run(run_id: str) -> dict:
    if not re.fullmatch(r'[1-9][0-9]*', run_id):
        raise ValueError('Use a numeric CI workflow run ID')
    if os.environ.get('GITHUB_REPOSITORY') != REPOSITORY:
        raise ValueError('Release publishing is restricted to the product repository')
    if os.environ.get('GITHUB_REF') != 'refs/heads/main':
        raise ValueError('Dispatch the release workflow from main')
    commit = package.git('rev-parse', 'HEAD')
    if commit != os.environ.get('GITHUB_SHA'):
        raise ValueError('Workflow and source revisions differ')
    if api('git/ref/heads/main')['object']['sha'] != commit:
        raise ValueError('The release source must be the current main revision')
    run = api(f'actions/runs/{run_id}')
    if (run.get('path') != '.github/workflows/ci.yml' or run.get('event') != 'push'
            or run.get('head_branch') != 'main' or run.get('head_sha') != commit
            or run.get('status') != 'completed' or run.get('conclusion') != 'success'
            or run.get('repository', {}).get('full_name') != REPOSITORY):
        raise ValueError('The selected run is not successful CI for this main revision')
    jobs = api(f'actions/runs/{run_id}/jobs?filter=latest&per_page=100')['jobs']
    required = {f'Validate ({target})' for target in package.TARGETS} | {'PR policy', 'CI'}
    if {job['name'] for job in jobs} != required or any(
            job['status'] != 'completed' or job['conclusion'] != 'success' for job in jobs):
        raise ValueError('Every native build, install test and CI gate must pass')
    release_version = package.version()
    if '-' in release_version:
        raise ValueError('This workflow publishes final versions only')
    for name, value in [('commit', commit), ('version', release_version),
                        ('tag', 'v' + release_version)]:
        workflow_output(name, value)
    return {'commit': commit, 'version': release_version, 'tag': 'v' + release_version}


def compose(directory: Path, commit: str, release_version: str, create_summary=False) -> dict[str, str]:
    expected = {package.archive_name(release_version, target) for target in package.TARGETS}
    expected |= {name + '.sha256' for name in list(expected)}
    existing = {path.name for path in directory.iterdir()}
    if existing - {'SHA256SUMS'} != expected:
        raise ValueError('The artifact set must contain every supported native archive and checksum')
    checksums = {}
    for target in package.TARGETS:
        archive = directory / package.archive_name(release_version, target)
        if archive.is_symlink() or not archive.is_file():
            raise ValueError('Release artifacts must be regular files')
        manifest = package.verify(archive, commit)
        if manifest.get('dirty') is not False or manifest['version'] != release_version:
            raise ValueError('Release archives must come from clean, matching source')
        value = package.digest(archive.read_bytes())
        line = f'{value}  {archive.name}\n'
        checksum_file = archive.with_name(archive.name + '.sha256')
        if checksum_file.is_symlink() or checksum_file.read_text(encoding='utf-8') != line:
            raise ValueError('Archive checksum does not match the tested artifact')
        checksums[archive.name] = value
    summary = ''.join(f'{checksums[name]}  {name}\n' for name in sorted(checksums))
    summary_path = directory / 'SHA256SUMS'
    if create_summary:
        summary_path.write_text(summary, encoding='utf-8', newline='\n')
    elif summary_path.is_symlink() or summary_path.read_text(encoding='utf-8') != summary:
        raise ValueError('Release checksum manifest does not match')
    return {path.name: package.digest(path.read_bytes()) for path in directory.iterdir()}


def find_release(tag: str):
    for page in range(1, 101):
        releases = api(f'releases?per_page=100&page={page}')
        for release in releases:
            if release['tag_name'] == tag:
                return release
        if len(releases) < 100:
            return None
    raise ValueError('Could not resolve the release unambiguously')


def ensure_tag(commit: str, tag: str, required=False) -> None:
    reference = api(f'git/ref/tags/{tag}', missing=True)
    if reference is None:
        if required:
            raise ValueError('Published release tag is missing')
        return
    obj = reference['object']
    for _ in range(4):
        if obj['type'] == 'commit':
            if obj['sha'] != commit:
                raise ValueError('Existing release tag points to a different source revision')
            return
        if obj['type'] != 'tag':
            break
        obj = api(f'git/tags/{obj["sha"]}')['object']
    raise ValueError('Release tag does not resolve to the qualified commit')


def qualified_release(identifier: str, source: dict, allow_published=False) -> dict:
    if not re.fullmatch(r'[1-9][0-9]*', identifier):
        raise ValueError('Invalid draft release ID')
    release = api(f'releases/{identifier}')
    if (not release['draft'] and not allow_published or release['tag_name'] != source['tag']
            or release['target_commitish'] != source['commit'] or release['prerelease']):
        raise ValueError('The draft does not match the qualified release source')
    ensure_tag(source['commit'], source['tag'], required=not release['draft'])
    return release


def verify_assets(release: dict, directory: Path, checksums: dict, complete=True) -> None:
    assets = {asset['name']: asset for asset in api(f'releases/{release["id"]}/assets?per_page=100')}
    if set(assets) - set(checksums) or complete and set(assets) != set(checksums):
        raise ValueError('Draft asset list does not match the qualified archive set')
    for name, asset in assets.items():
        if asset['state'] != 'uploaded' or asset['size'] != (directory / name).stat().st_size:
            raise ValueError('Draft asset upload is incomplete')
        if asset.get('digest') not in (None, 'sha256:' + checksums[name]):
            raise ValueError('Draft asset checksum differs from the qualified artifact')


def create_draft(directory: Path, run_id: str) -> None:
    source = verify_run(run_id)
    checksums = compose(directory, source['commit'], source['version'])
    ensure_tag(source['commit'], source['tag'])
    release = find_release(source['tag'])
    if release is None:
        notes = (package.ROOT / 'CHANGELOG.md').read_text(encoding='utf-8')
        notes += f'\nQualified source: `{source["commit"]}`.\n\n'
        notes += f'[Native builds and installation checks](https://github.com/{REPOSITORY}/actions/runs/{run_id}).\n'
        release = api('releases', 'POST', {'tag_name': source['tag'],
                      'target_commitish': source['commit'], 'name': 'Hudson CLI ' + source['tag'],
                      'body': notes, 'draft': True, 'prerelease': False})
    release = qualified_release(str(release['id']), source)
    verify_assets(release, directory, checksums, complete=False)
    existing = {asset['name'] for asset in api(f'releases/{release["id"]}/assets?per_page=100')}
    missing = [str(directory / name) for name in sorted(checksums) if name not in existing]
    if missing:
        subprocess.run(['gh', 'release', 'upload', source['tag'], '--repo', REPOSITORY, *missing], check=True)
    verify_assets(release, directory, checksums)
    workflow_output('release_id', str(release['id']))
    print('Draft contains the complete qualified artifact set')


def verify_download(directory: Path, run_id: str, identifier: str) -> None:
    source = verify_run(run_id)
    checksums = compose(directory, source['commit'], source['version'])
    release = qualified_release(identifier, source)
    verify_assets(release, directory, checksums)
    with tempfile.TemporaryDirectory(prefix='hudson uploaded release ') as temporary:
        destination = Path(temporary)
        subprocess.run(['gh', 'release', 'download', source['tag'], '--repo', REPOSITORY,
                        '--dir', str(destination)], check=True)
        downloaded = compose(destination, source['commit'], source['version'])
        if downloaded != checksums:
            raise ValueError('Downloaded draft bytes differ from the installed and tested archives')
    print('Downloaded draft assets match every installed and tested archive')


def publish(directory: Path, run_id: str, identifier: str) -> None:
    source = verify_run(run_id)
    checksums = compose(directory, source['commit'], source['version'])
    release = qualified_release(identifier, source, allow_published=True)
    verify_assets(release, directory, checksums)
    if not release['draft']:
        print(release['html_url'])
        return
    result = api(f'releases/{identifier}', 'PATCH', {'draft': False, 'make_latest': 'true'})
    if result['draft']:
        raise ValueError('Release was not published')
    ensure_tag(source['commit'], source['tag'], required=True)
    print(result['html_url'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['verify-run', 'compose', 'draft', 'verify-download', 'publish'])
    parser.add_argument('--ci-run', required=True)
    parser.add_argument('--directory', type=Path)
    parser.add_argument('--release-id')
    args = parser.parse_args()
    if args.action == 'verify-run':
        print(json.dumps(verify_run(args.ci_run)))
    elif args.action == 'compose':
        source = verify_run(args.ci_run)
        compose(args.directory, source['commit'], source['version'], create_summary=True)
        print('Complete native archive set verified')
    elif args.action == 'draft':
        create_draft(args.directory, args.ci_run)
    elif args.action == 'verify-download':
        verify_download(args.directory, args.ci_run, args.release_id)
    else:
        publish(args.directory, args.ci_run, args.release_id)


if __name__ == '__main__':
    main()
