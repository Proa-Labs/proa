#!/usr/bin/env python3
"""Copy a workspace example into a standalone consumer with local Proa paths."""
import argparse
import json
from pathlib import Path
import shutil
import tomllib


def literal(value):
    if isinstance(value, dict):
        return '{ ' + ', '.join(f'{key} = {literal(item)}' for key, item in value.items()) + ' }'
    if isinstance(value, bool):
        return str(value).lower()
    if isinstance(value, list):
        return '[' + ', '.join(literal(item) for item in value) + ']'
    return json.dumps(value)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('example')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    repo = Path(__file__).resolve().parent.parent
    source = repo / 'examples' / args.example
    workspace = tomllib.loads((repo / 'Cargo.toml').read_text())['workspace']
    manifest = tomllib.loads((source / 'Cargo.toml').read_text())
    shutil.copytree(source, args.output, ignore=shutil.ignore_patterns('target', '.git'))
    sections = {}
    for section, values in manifest.items():
        if section == 'lints':
            continue
        resolved = {}
        for key, value in values.items():
            if isinstance(value, dict) and value.get('workspace'):
                inherited = workspace['package' if section == 'package' else 'dependencies'][key]
                if isinstance(inherited, str) and section != 'package':
                    inherited = {'version': inherited}
                if isinstance(inherited, dict):
                    inherited = dict(inherited)
                    if 'path' in inherited:
                        inherited['path'] = str((repo / inherited['path']).resolve())
                    for option, setting in value.items():
                        if option == 'workspace':
                            continue
                        if option == 'features':
                            inherited[option] = list(dict.fromkeys(inherited.get(option, []) + setting))
                        else:
                            inherited[option] = setting
                if section == 'package' and key == 'license-file':
                    inherited = str(repo / inherited)
                value = inherited
            resolved[key] = value
        sections[section] = resolved
    text = '\n\n'.join('[' + section + ']\n' + '\n'.join(f'{key} = {literal(value)}' for key, value in values.items()) for section, values in sections.items())
    text += '\n\n[workspace]\n\n[profile.nodebug]\ninherits = "dev"\ndebug = 0\n'
    (args.output / 'Cargo.toml').write_text(text)
    shutil.copy(repo / 'rust-toolchain.toml', args.output / 'rust-toolchain.toml')
    shutil.copy(repo / 'Cargo.lock', args.output / 'Cargo.lock')
    print(args.output.resolve())


if __name__ == '__main__':
    main()
