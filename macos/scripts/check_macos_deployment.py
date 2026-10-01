#!/usr/bin/env python3
"""Fail packaging if ANY Mach-O slice exceeds the product's macOS minimum."""
import argparse
import json
import plistlib
import re
import subprocess
from pathlib import Path

MAGICS = {bytes.fromhex(x) for x in ('feedface', 'cefaedfe', 'feedfacf', 'cffaedfe', 'cafebabe', 'bebafeca', 'cafebabf', 'bfbafeca')}

def version(value):
    return tuple((list(map(int, value.split('.'))) + [0, 0])[:3])

def minimums(load_commands):
    values = []
    for block in re.split(r'Load command \d+', load_commands):
        if re.search(r'cmd LC_BUILD_VERSION\b', block):
            match = re.search(r'^\s*minos\s+(\d+(?:\.\d+){0,2})\s*$', block, re.M)
        elif re.search(r'cmd LC_VERSION_MIN_MACOSX\b', block):
            match = re.search(r'^\s*version\s+(\d+(?:\.\d+){0,2})\s*$', block, re.M)
        else:
            continue
        if match:
            values.append(match.group(1))
    if not values:
        raise ValueError('missing deployment load command')
    return values

def check(app, expected=None):
    with (app / 'Contents/Info.plist').open('rb') as source:
        declared = plistlib.load(source)['LSMinimumSystemVersion']
    if version(declared) != version('13.0'):
        raise ValueError(f'product declaration must be macOS 13.0, got {declared}')
    report = []
    for path in sorted(app.rglob('*')):
        if path.is_symlink() or not path.is_file():
            continue
        with path.open('rb') as source:
            if source.read(4) not in MAGICS:
                continue
        arches = subprocess.check_output(['lipo', '-archs', str(path)], text=True).split()
        if expected and set(arches) != set(expected):
            raise ValueError(f'{path}: architectures {arches}, expected {expected}')
        for arch in arches:
            text = subprocess.check_output(['otool', '-arch', arch, '-l', str(path)], text=True)
            values = minimums(text)
            if any(version(value) > version(declared) for value in values):
                raise ValueError(f'{path} [{arch}] minos {values} exceeds {declared}')
            report.append(dict(path=str(path.relative_to(app)), architecture=arch, minos=values))
    required = {'Contents/MacOS/TailSync', 'Contents/MacOS/tailsyncd', 'Contents/MacOS/clipboard-helper'}
    if not required.issubset({row['path'] for row in report}):
        raise ValueError('required product Mach-O binary missing')
    return report

if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('app', type=Path)
    parser.add_argument('--architectures')
    args = parser.parse_args()
    expected = ['arm64', 'x86_64'] if args.architectures == 'universal-apple-darwin' else args.architectures.split(',') if args.architectures else None
    try:
        print(json.dumps(dict(status='PASS', binaries=check(args.app, expected)), indent=2))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f'macOS deployment guard: {error}\n')
