"""Check local guide links and versioned release documentation."""
from pathlib import Path
import re
from urllib.parse import unquote

import package


def main():
    files = [package.ROOT / name for name in ['README.md','CONTRIBUTING.md','SECURITY.md',
                                             'AGENTS.md','CHANGELOG.md']]
    files += sorted((package.ROOT/'docs').rglob('*.md'))
    for path in files:
        text = path.read_text(encoding='utf-8')
        for link in re.findall(r'\[[^\]]+\]\(([^)]+)\)',text):
            target = link.split('#',1)[0]
            if not target or re.match(r'^[a-z]+:',target):
                continue
            destination = (path.parent/unquote(target)).resolve()
            if not destination.is_relative_to(package.ROOT) or not destination.exists():
                raise ValueError(f'Broken guide link in {path.relative_to(package.ROOT)}: {link}')
    if f'## {package.version()}' not in (package.ROOT/'CHANGELOG.md').read_text():
        raise ValueError('Changelog does not describe the current release version')
    for name in package.CURRENT_DOCS:
        if not (package.ROOT/'docs'/name).is_file():
            raise ValueError('Missing release guide: '+name)
    print(f'Guide links and release version passed: {len(files)} Markdown files')


if __name__ == '__main__':
    main()
