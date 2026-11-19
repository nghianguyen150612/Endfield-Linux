#!/usr/bin/env python3
"""Inspect a separately acquired Wine source archive without extracting it.

A matching signature establishes correspondence to the supplied key only. The
caller must independently authenticate that key before trusting its identity.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import subprocess
import tarfile
import tempfile

MAX_ARCHIVE = 50_000_000
MAX_EXPANDED = 1024 * 1024 * 1024
MAX_ENTRIES = 100_000


def digest(path):
    h = hashlib.sha256()
    size = 0
    with path.open('rb') as source:
        while chunk := source.read(1024 * 1024):
            size += len(chunk)
            if size > MAX_ARCHIVE:
                raise ValueError('input exceeds bounded size')
            h.update(chunk)
    return h.hexdigest()


def inspect(archive):
    names = set()
    total = 0
    with tarfile.open(archive, mode='r:xz') as source:
        for entry in source:
            entry.name.encode('utf-8', errors='strict')
            parts = PurePosixPath(entry.name).parts
            if (not parts or parts[0] != 'wine-10.0' or entry.name.startswith('/')
                    or any(part in ('', '.', '..') for part in entry.name.rstrip('/').split('/'))
                    or '\\' in entry.name or any(ord(char) < 32 or ord(char) == 127 for char in entry.name)
                    or not (entry.isfile() or entry.isdir())
                    or entry.mode & 0o6000 or entry.name in names):
                raise ValueError('unsafe or duplicate source archive entry')
            names.add(entry.name)
            total += entry.size
            if len(names) > MAX_ENTRIES or total > MAX_EXPANDED:
                raise ValueError('source archive expansion limit exceeded')
    if 'wine-10.0/configure.ac' not in names:
        raise ValueError('source archive has no configure.ac')
    return len(names), total


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('archive', type=Path)
    parser.add_argument('signature', type=Path)
    parser.add_argument('public_key', type=Path)
    parser.add_argument('--expected-sha256', required=True)
    parser.add_argument('--expected-fingerprint', required=True)
    args = parser.parse_args()
    if not re.fullmatch(r'[0-9a-f]{64}', args.expected_sha256):
        parser.error('expected SHA-256 must be lowercase hexadecimal')
    if not re.fullmatch(r'[0-9A-F]{40}', args.expected_fingerprint):
        parser.error('expected fingerprint must be 40 uppercase hexadecimal characters')
    actual = digest(args.archive)
    if actual != args.expected_sha256:
        raise ValueError('source SHA-256 mismatch')
    with tempfile.TemporaryDirectory(prefix='wine-source-verification-') as home:
        os.chmod(home, 0o700)
        env = {'PATH': '/usr/bin:/bin', 'GNUPGHOME': home, 'LC_ALL': 'C'}
        subprocess.run(['gpg', '--batch', '--import', str(args.public_key)], env=env,
                       check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        result = subprocess.run(['gpg', '--batch', '--status-fd=1', '--verify',
                                 str(args.signature), str(args.archive)], env=env,
                                check=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                text=True)
        valid = [line.split() for line in result.stdout.splitlines()
                 if line.startswith('[GNUPG:] VALIDSIG ')]
        if len(valid) != 1 or valid[0][2] != args.expected_fingerprint:
            raise ValueError('signature fingerprint differs from expected signer')
    count, expanded = inspect(args.archive)
    print(json.dumps({'schema_version': 1, 'source': 'wine-10.0',
                      'archive_sha256': actual, 'signature_fingerprint': args.expected_fingerprint,
                      'signature_state': 'VALID_FOR_SUPPLIED_KEY',
                      'signer_identity': 'NOT_INDEPENDENTLY_AUTHENTICATED',
                      'archive_entries': count, 'expanded_bytes': expanded,
                      'binary_provenance': 'NOT_AVAILABLE', 'authentic_execution': 'NOT_RUN'},
                     indent=2))


if __name__ == '__main__':
    main()
