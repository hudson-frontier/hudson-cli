"""Render the command reference from offline help for the built executable."""
import argparse
import os
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parents[1]


def render(binary: Path) -> str:
    environment = dict(os.environ, NO_COLOR='1', TERM='dumb', COLUMNS='100')
    environment.pop('HUDSONBOX_CONFIG', None)
    queue = [()]
    seen = set()
    pages = []
    while queue:
        path = queue.pop(0)
        if path in seen:
            continue
        if len(path) > 6 or len(seen) > 200:
            raise ValueError('Unexpected command tree')
        seen.add(path)
        result = subprocess.run([str(binary), *path, '--help'], check=True,
                                capture_output=True, text=True, env=environment, timeout=15)
        if result.stderr:
            raise ValueError('Help wrote diagnostics while generating the reference')
        help_text = '\n'.join(line.rstrip() for line in result.stdout.replace('\r\n', '\n').splitlines()).strip()
        pages.append('## ' + ' '.join(['hudson', *path]) + '\n\n```text\n' + help_text + '\n```')
        section = re.search(r'(?ms)^Commands:\n(.*?)(?=\n\S[^\n]*:|\Z)', help_text)
        if section:
            children = re.findall(r'^  ([a-z][a-z0-9-]*)(?:[ \t]|$)', section[1], re.MULTILINE)
            queue.extend((*path, name) for name in children if name != 'help')
    return ('# Command reference\n\n'
            'Generated from the built CLI with `--help`. Help is available offline and\n'
            'does not load service credentials. See the task guides for complete workflows.\n\n'
            + '\n\n'.join(pages) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--output', type=Path, default=ROOT / 'docs/reference.md')
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    expected = render(args.binary.resolve())
    if args.check:
        if not args.output.is_file() or args.output.read_text(encoding='utf-8') != expected:
            raise SystemExit('Command reference is stale; regenerate it from the built CLI')
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(expected, encoding='utf-8', newline='\n')
    print('Command reference is current')


if __name__ == '__main__':
    main()
