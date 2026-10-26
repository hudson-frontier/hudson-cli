"""Build, test and freshly pour a native Homebrew bottle without publishing it."""
from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

import homebrew
import package


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', required=True, choices=sorted(homebrew.SUPPORTED_TARGETS))
    parser.add_argument('--output', type=Path, default=package.ROOT / 'dist-homebrew')
    args = parser.parse_args()
    output = args.output.resolve()
    receipt = homebrew.prepare(output)
    environment = dict(os.environ, HOMEBREW_NO_AUTO_UPDATE='1', HOMEBREW_NO_ANALYTICS='1')

    def run(*command, capture=False):
        print('Running:', ' '.join(map(str, command)), flush=True)
        return subprocess.run(list(map(str, command)), check=True, env=environment,
                              cwd=output, capture_output=capture, text=True)

    # The CI machine owns this temporary tap. Never replace an installed tap.
    run('brew', 'tap-new', '--no-git', homebrew.TAP)
    tap = Path(run('brew', '--repository', homebrew.TAP, capture=True).stdout.strip())
    formula = tap / 'Formula/hudson.rb'
    if formula.exists() or formula.is_symlink():
        raise ValueError('The CI tap must not already contain a Hudson formula')
    shutil.copyfile(output / 'hudson.rb', formula)
    # Bottle metadata records a real tap revision without touching global Git
    # configuration or pushing anything to the future distribution repository.
    run('git', '-C', tap, 'init', '--initial-branch=main')
    run('git', '-C', tap, 'config', 'user.name', 'Hudson Homebrew CI')
    run('git', '-C', tap, 'config', 'user.email', 'homebrew-ci@hudson.invalid')
    run('git', '-C', tap, 'config', 'commit.gpgsign', 'false')
    run('git', '-C', tap, 'add', 'Formula/hudson.rb')
    run('git', '-C', tap, 'commit', '-m', 'Build the qualified Hudson formula')
    run('git', '-C', tap, 'remote', 'add', 'origin', 'https://github.com/hudson-frontier/homebrew-tap')
    run('brew', 'trust', '--formula', homebrew.FORMULA)
    run('brew', 'style', homebrew.FORMULA)
    run('brew', 'audit', '--strict', '--formula', homebrew.FORMULA)
    run('brew', 'install', '--build-bottle', '--verbose', homebrew.FORMULA)
    run('brew', 'test', homebrew.FORMULA)
    root_url = f'https://github.com/hudson-frontier/homebrew-tap/releases/download/v{receipt["version"]}'
    run('brew', 'bottle', '--json', '--no-rebuild', '--root-url=' + root_url, homebrew.FORMULA)
    manifests = list(output.glob('*.bottle.json'))
    if len(manifests) != 1:
        raise ValueError('Expected exactly one native bottle manifest')
    value = json.loads(manifests[0].read_text())[homebrew.FORMULA]
    formula_info, bottle = value['formula'], value['bottle']
    if (formula_info['pkg_version'] != receipt['version'] or bottle['root_url'] != root_url
            or bottle['rebuild'] != 0 or len(bottle['tags']) != 1):
        raise ValueError('Bottle does not match the selected source/version')
    tag, spec = next(iter(bottle['tags'].items()))
    native = homebrew.verify_bottle(output, receipt, args.target, tag, spec, bottle['cellar'])
    run('brew', 'uninstall', '--force', homebrew.FORMULA)
    run('brew', 'install', '--force-bottle', native)
    run('brew', 'test', homebrew.FORMULA)
    installed = json.loads(run('brew', 'info', '--json=v2', homebrew.FORMULA, capture=True).stdout)
    records = installed['formulae'][0]['installed']
    if len(records) != 1 or records[0]['version'] != receipt['version'] or not records[0]['poured_from_bottle']:
        raise ValueError('Fresh installation did not pour the tested native bottle')
    receipt.update({'target': args.target, 'bottle_tag': tag, 'cellar': bottle['cellar'],
                    'bottle_file': spec['filename'], 'bottle_sha256': spec['sha256'],
                    'poured_from_bottle': True})
    package.publish_checksum(output / 'verified-bottle.json', json.dumps(receipt, indent=2, sort_keys=True) + '\n')
    print('Native Homebrew source build, functional test and fresh bottle installation passed', flush=True)


if __name__ == '__main__':
    main()
