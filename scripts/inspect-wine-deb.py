#!/usr/bin/env python3
"""Offline, non-executing DEB inventory. No scripts, links or executable files extracted.

Input digest is integrity evidence only; repository authentication is separate.
Output must be a new directory beneath a private, user-owned parent.
"""
import argparse
import hashlib
import gzip
import lzma
import io
import json
import os
from pathlib import Path
import tarfile
import subprocess
import tempfile

MAX_PACKAGE = 300_000_000
MAX_EXPANDED = 2_000_000_000
MAX_FILE = 100_000_000
MAX_ENTRIES = 30_000


def sha(data):
    return hashlib.sha256(data).hexdigest()


def encode(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode()


def path_name(raw):
    if raw.startswith('./'):
        raw = raw[2:]
    raw = raw.rstrip('/')
    if (not raw or len(raw) > 4096 or raw.startswith('/') or '\\' in raw
            or any(ord(c) < 32 or ord(c) == 127 for c in raw)
            or any(p in ('', '.', '..') for p in raw.split('/'))):
        raise ValueError('unsafe archive path')
    return raw


def ar_members(data):
    if not data.startswith(b'!<arch>\n'):
        raise ValueError('invalid ar magic')
    result = {}
    pos = 8
    while pos < len(data):
        header = data[pos:pos+60]
        if len(header) != 60 or header[58:] != b'`\n':
            raise ValueError('invalid ar header')
        name = header[:16].decode('ascii').strip().removesuffix('/')
        size = int(header[48:58])
        pos += 60
        if size < 0 or size > MAX_PACKAGE or pos + size > len(data) or name in result:
            raise ValueError('invalid ar member')
        if name not in ('debian-binary', 'control.tar.xz', 'control.tar.gz', 'data.tar.xz', 'data.tar.gz', 'data.tar.zst', 'control.tar.zst'):
            raise ValueError('unsupported ar member')
        result[name] = data[pos:pos+size]
        pos += size + size % 2
    if pos != len(data) or result.get('debian-binary') != b'2.0\n' or len(result) != 3:
        raise ValueError('invalid deb structure')
    return result


def inventory(data, destination=None):
    if len(data) > MAX_PACKAGE:
        raise ValueError('compressed stream limit')
    if data.startswith(b'\x28\xb5\x2f\xfd'):
        with tempfile.TemporaryFile() as compressed, tempfile.TemporaryFile() as expanded:
            compressed.write(data)
            compressed.seek(0)
            with subprocess.Popen(['zstd', '-dc'], stdin=compressed, stdout=subprocess.PIPE,
                                  stderr=subprocess.DEVNULL) as process:
                count = 0
                while chunk := process.stdout.read(1024 * 1024):
                    count += len(chunk)
                    if count > MAX_EXPANDED:
                        process.kill()
                        raise ValueError('decoded stream limit')
                    expanded.write(chunk)
                if process.wait() != 0:
                    raise ValueError('invalid zstd stream')
            expanded.seek(0)
            return inventory_tar(expanded, destination)
    stream = io.BytesIO(data)
    if data.startswith(b'\x1f\x8b'):
        with gzip.GzipFile(fileobj=stream) as decoded:
            return inventory_tar(decoded, destination)
    if data.startswith(b'\xfd7zXZ\x00'):
        with lzma.LZMAFile(stream) as decoded:
            return inventory_tar(decoded, destination)
    return inventory_tar(stream, destination)


class BoundedReader:
    def __init__(self, stream):
        self.stream = stream
        self.remaining = MAX_EXPANDED

    def read(self, size):
        if size < 0 or size > MAX_FILE or size > self.remaining:
            raise ValueError('decoded stream limit')
        result = self.stream.read(size)
        self.remaining -= len(result)
        return result


def inventory_tar(stream, destination=None):
    entries, seen, files, parents = [], set(), set(), set()
    total = 0
    # Streaming avoids unbounded metadata accumulation. Reject extension records.
    with tarfile.open(fileobj=BoundedReader(stream), mode='r|') as archive:
        for item in archive:
            if item.name in ('.', './') and item.isdir():
                continue
            name = path_name(item.name)
            ancestors = {'/'.join(name.split('/')[:i]) for i in range(1, len(name.split('/')))}
            if (name in seen or files.intersection(ancestors) or item.mode & 0o6000
                    or not (item.isfile() or item.isdir() or item.issym())
                    or item.pax_headers or (not item.isdir() and name in parents)):
                raise ValueError('unsafe, duplicate or unsupported tar entry')
            seen.add(name)
            parents.update(ancestors)
            total += item.size
            if len(seen) > MAX_ENTRIES or item.size > MAX_FILE or total > MAX_EXPANDED:
                raise ValueError('expansion limit')
            if item.isdir():
                if item.size:
                    raise ValueError('directory content')
                kind = {'type': 'directory', 'mode': 448}
            elif item.issym():
                # Preserve evidence only. Never materialize links, including safe links.
                path_name(item.linkname)
                if item.size:
                    raise ValueError('symlink content')
                files.add(name)
                kind = {'type': 'symlink', 'target': item.linkname}
            else:
                files.add(name)
                content = archive.extractfile(item).read(MAX_FILE + 1)
                if len(content) != item.size:
                    raise ValueError('truncated entry')
                kind = {'type': 'file', 'size': len(content), 'sha256': sha(content),
                        'executable': bool(item.mode & 0o111), 'setid': False}
                if destination:
                    out = destination / name
                    out.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                    with out.open('xb') as target:
                        target.write(content)
                    out.chmod(0o600)
            entries.append({'path': name, 'kind': kind})
    return sorted(entries, key=lambda e: e['path'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('package', type=Path)
    parser.add_argument('destination', type=Path)
    parser.add_argument('--expected-sha256', required=True)
    args = parser.parse_args()
    parent = args.destination.parent.resolve(strict=True)
    st = parent.stat()
    if args.destination.parent != parent or st.st_uid != os.geteuid() or st.st_mode & 0o077:
        parser.error('private absolute user-owned parent required')
    if args.package.stat().st_size > MAX_PACKAGE:
        raise ValueError('package limit')
    data = args.package.read_bytes()
    if sha(data) != args.expected_sha256:
        raise ValueError('package digest mismatch')
    members = ar_members(data)
    payload = next(v for k, v in members.items() if k.startswith('data.tar.'))
    control = next(v for k, v in members.items() if k.startswith('control.tar.'))
    # Validate all entries before writing any extracted payload.
    entries = inventory(payload)
    controls = inventory(control)
    args.destination.mkdir(mode=0o700)
    staging = args.destination / 'staging'
    staging.mkdir(mode=0o700)
    assert inventory(payload, staging) == entries
    report = {'schema_version': 1, 'package_sha256': sha(data),
              'inventory_sha256': sha(encode(entries)), 'entries': entries,
              'control_entries': controls, 'staging_executable': False,
              'publisher_identity': 'UNVERIFIED', 'authentic_execution': 'NOT_RUN'}
    (args.destination / 'inventory.json').write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps({k: v for k, v in report.items() if k not in ('entries', 'control_entries')}))


if __name__ == '__main__':
    main()
