"""Audit a composed formula in a temporary CI tap without publishing it."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess

import homebrew


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--formula', required=True, type=Path)
    args = parser.parse_args()
    source = args.formula.resolve(strict=True)
    environment = dict(os.environ, HOMEBREW_NO_AUTO_UPDATE='1', HOMEBREW_NO_ANALYTICS='1')

    def run(*command, capture=False):
        print('Running:', ' '.join(map(str, command)), flush=True)
        return subprocess.run(list(map(str, command)), check=True, env=environment,
                              capture_output=capture, text=True)

    run('brew', 'tap-new', '--no-git', homebrew.TAP)
    tap = Path(run('brew', '--repository', homebrew.TAP, capture=True).stdout.strip())
    formula = tap / 'Formula/hudson.rb'
    if formula.exists() or formula.is_symlink():
        raise ValueError('The CI audit tap must not already contain a Hudson formula')
    # Preserve permissions so the audit checks the publication output itself.
    shutil.copy2(source, formula)
    run('git', '-C', tap, 'init', '--initial-branch=main')
    run('git', '-C', tap, 'config', 'user.name', 'Hudson Homebrew CI')
    run('git', '-C', tap, 'config', 'user.email', 'homebrew-ci@hudson.invalid')
    run('git', '-C', tap, 'config', 'commit.gpgsign', 'false')
    run('git', '-C', tap, 'add', 'Formula/hudson.rb')
    run('git', '-C', tap, 'commit', '-m', 'Audit the composed Hudson formula')
    run('git', '-C', tap, 'remote', 'add', 'origin', 'https://github.com/hudson-frontier/homebrew-tap')
    run('brew', 'trust', '--formula', homebrew.FORMULA)
    run('brew', 'style', homebrew.FORMULA)
    run('brew', 'audit', '--strict', '--formula', homebrew.FORMULA)


if __name__ == '__main__':
    main()
