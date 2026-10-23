"""Validate a native release build and then test its extracted archive."""
import argparse
import os
from pathlib import Path
import platform
import subprocess
import sys

import package


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--target', default=os.environ.get('PACKAGE_TARGET'), choices=package.TARGETS)
    parser.add_argument('--allow-dirty', action='store_true')
    args = parser.parse_args()
    if args.target not in package.TARGETS:
        raise SystemExit('Select a supported native release target')
    system, architecture = package.TARGETS[args.target]
    host_system = {'Linux': 'linux', 'Darwin': 'macos', 'Windows': 'windows'}.get(platform.system())
    host_architecture = {'AMD64': 'x86_64', 'x86_64': 'x86_64', 'arm64': 'aarch64',
                         'aarch64': 'aarch64'}.get(platform.machine())
    if (system, architecture) != (host_system, host_architecture):
        raise SystemExit('Release and installation checks must run on the native target')
    environment = dict(os.environ)
    environment.setdefault('CARGO_BUILD_JOBS', '2')
    if os.environ.get('GITHUB_ACTIONS') == 'true':
        environment['RUSTUP_HOME'] = str(Path(os.environ['RUNNER_TEMP']) / 'hudson-cli-rustup')
    if system == 'windows':
        environment['RUSTFLAGS'] = '-C target-feature=+crt-static'
    if system == 'macos':
        environment['MACOSX_DEPLOYMENT_TARGET'] = '12.0'

    def run(*command):
        print('Running:', ' '.join(map(str, command)), flush=True)
        subprocess.run(list(map(str, command)), cwd=package.ROOT, env=environment, check=True)

    run('rustup', 'show', 'active-toolchain')
    run('cargo', 'fmt', '--all', '--', '--check')
    run('cargo', 'clippy', '--all-targets', '--locked', '--target', args.target, '--', '-D', 'warnings')
    run('cargo', 'build', '--bins', '--release', '--locked', '--target', args.target)
    run('cargo', 'test', '--all-targets', '--release', '--locked', '--target', args.target)
    target_dir = Path(environment.get('CARGO_TARGET_DIR', package.ROOT / 'target'))
    if not target_dir.is_absolute():
        target_dir = package.ROOT / target_dir
    binary_dir = target_dir / args.target / 'release'
    if system == 'macos':
        for name in package.BINARIES:
            run('codesign', '--force', '--sign', '-', '--timestamp=none', binary_dir / name)
            run('codesign', '--verify', '--strict', binary_dir / name)
    if args.target == 'x86_64-unknown-linux-gnu':
        run(sys.executable, 'scripts/generate_reference.py', '--binary', binary_dir / 'hudson', '--check')
    build_args = [sys.executable, 'scripts/package.py', 'build', '--target', args.target,
                  '--binary-dir', str(binary_dir), '--output', 'dist']
    if args.allow_dirty:
        build_args.append('--allow-dirty')
    run(*build_args)
    archive = package.ROOT / 'dist' / package.archive_name(package.version(), args.target)
    run(sys.executable, 'scripts/package.py', 'check', '--archive', archive,
        '--commit', package.git('rev-parse', 'HEAD'), '--install-test')
    print('Native release archive and clean-directory installation passed', flush=True)


if __name__ == '__main__':
    main()
