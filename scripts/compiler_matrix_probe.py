#!/usr/bin/env python3
"""Run compiler experiments serially, retaining isolated caches and raw output."""
import argparse
import json
from pathlib import Path
import subprocess
import sys

CONFIGURATIONS = [
    ('rust195', '1.95.0', ''),
    ('rust199', '1.99.0', ''),
    ('nightly', 'nightly-2026-08-01', ''),
    ('solver', 'nightly-2026-08-01', '-Znext-solver=globally'),
    ('frontend4', 'nightly-2026-08-01', '-Zthreads=4'),
    ('cranelift', 'nightly-2026-08-01', '-Zcodegen-backend=cranelift -Cpanic=abort'),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--fixture', type=Path, required=True)
    parser.add_argument('--cli', type=Path, required=True)
    parser.add_argument('--configurations', nargs='+', default=[row[0] for row in CONFIGURATIONS])
    parser.add_argument('--profile', default='nodebug')
    parser.add_argument('--samples', default='3')
    parser.add_argument('--cases', nargs='+', default=['linked-ssr', 'linked-islands'])
    parser.add_argument('--suffix', default='', help='Isolate a second comparison, e.g. -direct')
    args = parser.parse_args()
    scripts = Path(__file__).resolve().parent
    output = args.fixture / ('target/compiler-matrix' + args.suffix)
    output.mkdir(parents=True, exist_ok=True)
    results = []
    for name, toolchain, flags in CONFIGURATIONS:
        if name not in args.configurations:
            continue
        configuration = name + args.suffix
        command = [sys.executable, str(scripts / 'compile_time_probe.py'), '--fixture', str(args.fixture),
                   '--cli', str(args.cli.resolve()), '--profiles', args.profile, '--cases', *args.cases,
                   '--samples', args.samples, '--configuration', configuration, '--toolchain', toolchain, f'--rustflags={flags}']
        print(f'Compiler configuration: {name} ({toolchain}, {flags or "default flags"})', flush=True)
        process = subprocess.run(command, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        (output / f'{name}.txt').write_text(process.stdout)
        records = args.fixture / f'target/{configuration}-results.json'
        row = {'configuration': configuration, 'toolchain': toolchain, 'rustflags': flags, 'exit_code': process.returncode,
               'log': str(output / f'{name}.txt')}
        if process.returncode == 0:
            row['results'] = json.loads(records.read_text())
        else:
            row['failure'] = process.stdout[-6000:]
        results.append(row)
        (output / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
        print(json.dumps({key: value for key, value in row.items() if key != 'results'}), flush=True)
    print(output / 'results.json')


if __name__ == '__main__':
    main()
