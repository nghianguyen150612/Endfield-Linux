#!/usr/bin/env python3
"""Explicit Wine 10.0 source acquisition into a new disposable directory."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import urllib.request

BASE = 'https://dl.winehq.org/wine/source/10.0/'
FILES = (('wine-10.0.tar.xz', 50_000_000), ('wine-10.0.tar.xz.sign', 65_536))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path, help='new directory under a private parent')
    parser.add_argument('--public-key', required=True, type=Path)
    parser.add_argument('--expected-sha256', required=True)
    parser.add_argument('--expected-fingerprint', required=True)
    args = parser.parse_args()
    if os.getuid() == 0 or os.getuid() != os.geteuid():
        parser.error('acquisition requires an unprivileged user')
    root = args.destination
    parent = root.parent.resolve(strict=True)
    metadata = parent.stat()
    if not root.is_absolute() or root.parent != parent or metadata.st_uid != os.geteuid() or metadata.st_mode & 0o077:
        parser.error('destination parent must be an absolute private user-owned directory without symlink aliases')
    root.mkdir(mode=0o700)
    try:
        for name, maximum in FILES:
            request = urllib.request.Request(BASE + name, headers={'User-Agent': 'Endfield-Unix-source-inspection/1'})
            with urllib.request.urlopen(request, timeout=60) as response:
                if response.geturl() != BASE + name:
                    raise ValueError('redirect is not accepted')
                length = response.headers.get('Content-Length')
                if length and int(length) > maximum:
                    raise ValueError('declared download exceeds limit')
                count = 0
                with (root / name).open('xb') as output:
                    while chunk := response.read(1024 * 1024):
                        count += len(chunk)
                        if count > maximum:
                            raise ValueError('download exceeds limit')
                        output.write(chunk)
                    output.flush()
                    os.fsync(output.fileno())
        inspector = Path(__file__).with_name('inspect-wine-source.py')
        command = [sys.executable, str(inspector), str(root / FILES[0][0]), str(root / FILES[1][0]),
                   str(args.public_key), '--expected-sha256', args.expected_sha256,
                   '--expected-fingerprint', args.expected_fingerprint]
        result = subprocess.run(command, check=True, capture_output=True, text=True)
        evidence = json.loads(result.stdout)
        evidence['source_url'] = BASE + FILES[0][0]
        evidence['signature_url'] = BASE + FILES[1][0]
        with (root / 'evidence.json').open('x', encoding='utf-8') as output:
            json.dump(evidence, output, indent=2)
            output.write('\n')
            output.flush()
            os.fsync(output.fileno())
        directory_fd = os.open(root, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory_fd)
        finally:
            os.close(directory_fd)
    except Exception as error:
        with (root / 'failure.json').open('x', encoding='utf-8') as output:
            json.dump({'schema_version': 1, 'state': 'ACQUISITION_FAILED', 'error': str(error),
                       'authentic_execution': 'NOT_RUN'}, output, indent=2)
            output.flush()
            os.fsync(output.fileno())
        raise


if __name__ == '__main__':
    main()
