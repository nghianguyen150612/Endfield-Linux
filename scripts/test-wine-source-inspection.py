#!/usr/bin/env python3
"""Offline adversarial tests for the Wine source archive inspector."""
import importlib.util
import io
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path
import tarfile
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('wine_source', Path(__file__).with_name('inspect-wine-source.py'))
inspector = importlib.util.module_from_spec(spec)
spec.loader.exec_module(inspector)


class SourceArchiveTests(unittest.TestCase):
    def test_safe_and_unsafe_entries(self):
        for name, kind, accepted in [
            ('wine-10.0/configure.ac', tarfile.REGTYPE, True),
            ('wine-10.0/../../escape', tarfile.REGTYPE, False),
            ('/absolute', tarfile.REGTYPE, False),
            ('wine-10.0/link', tarfile.SYMTYPE, False),
            ('wine-10.0/hardlink', tarfile.LNKTYPE, False),
            ('wine-10.0/device', tarfile.CHRTYPE, False),
        ]:
            with self.subTest(name=name), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'fixture.tar.xz'
                with tarfile.open(path, 'w:xz') as archive:
                    entry = tarfile.TarInfo(name)
                    entry.type = kind
                    entry.linkname = '../../escape'
                    archive.addfile(entry, io.BytesIO(b''))
                    if not accepted:
                        archive.addfile(tarfile.TarInfo('wine-10.0/configure.ac'), io.BytesIO(b''))
                if accepted:
                    self.assertEqual(inspector.inspect(path), (1, 0))
                else:
                    with self.assertRaises(ValueError):
                        inspector.inspect(path)

    def test_duplicate_names(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'fixture.tar.xz'
            with tarfile.open(path, 'w:xz') as archive:
                for _ in range(2):
                    archive.addfile(tarfile.TarInfo('wine-10.0/configure.ac'), io.BytesIO(b''))
            with self.assertRaises(ValueError):
                inspector.inspect(path)

    def test_file_parent_conflicts_in_either_order(self):
        for names in [('wine-10.0/sub', 'wine-10.0/sub/file'),
                      ('wine-10.0/sub/file', 'wine-10.0/sub')]:
            with self.subTest(names=names), tempfile.TemporaryDirectory() as directory:
                path = Path(directory) / 'fixture.tar.xz'
                with tarfile.open(path, 'w:xz') as archive:
                    archive.addfile(tarfile.TarInfo('wine-10.0/configure.ac'), io.BytesIO(b''))
                    for name in names:
                        archive.addfile(tarfile.TarInfo(name), io.BytesIO(b''))
                with self.assertRaises(ValueError):
                    inspector.inspect(path)

    def test_signature_digest_and_signer_binding(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / 'gnupg'
            home.mkdir(mode=0o700)
            env = dict(os.environ, GNUPGHOME=str(home), LC_ALL='C')
            def gpg(*args):
                return subprocess.run(['gpg', '--batch', '--yes', *args], env=env,
                                      check=True, capture_output=True)
            gpg('--passphrase', '', '--quick-generate-key', 'Fixture <fixture@example.invalid>',
                'ed25519', 'sign', '0')
            listing = gpg('--with-colons', '--list-keys').stdout.decode()
            fingerprint = next(line.split(':')[9] for line in listing.splitlines()
                               if line.startswith('fpr:'))
            key = root / 'key.gpg'
            key.write_bytes(gpg('--export').stdout)
            archive = root / 'fixture.tar.xz'
            with tarfile.open(archive, 'w:xz') as output:
                output.addfile(tarfile.TarInfo('wine-10.0/configure.ac'), io.BytesIO(b''))
            signature = root / 'fixture.sign'
            gpg('--output', str(signature), '--detach-sign', str(archive))
            expected_hash = hashlib.sha256(archive.read_bytes()).hexdigest()
            command = [sys.executable, str(Path(__file__).with_name('inspect-wine-source.py')),
                       str(archive), str(signature), str(key), '--expected-sha256', expected_hash,
                       '--expected-fingerprint', fingerprint]
            result = subprocess.run(command, capture_output=True, text=True, check=True)
            evidence = json.loads(result.stdout)
            self.assertEqual(evidence['signature_state'], 'VALID_FOR_SUPPLIED_KEY')
            self.assertEqual(evidence['signer_identity'], 'NOT_INDEPENDENTLY_AUTHENTICATED')
            wrong_key = command[:-1] + ['0' * 40]
            self.assertNotEqual(subprocess.run(wrong_key, capture_output=True).returncode, 0)
            signature.write_bytes(b'forged')
            self.assertNotEqual(subprocess.run(command, capture_output=True).returncode, 0)
            wrong_hash = command.copy()
            wrong_hash[wrong_hash.index('--expected-sha256') + 1] = '0' * 64
            self.assertNotEqual(subprocess.run(wrong_hash, capture_output=True).returncode, 0)


if __name__ == '__main__':
    unittest.main()
