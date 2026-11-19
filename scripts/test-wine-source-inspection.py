#!/usr/bin/env python3
"""Offline adversarial tests for the Wine source archive inspector."""
import importlib.util
import io
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


if __name__ == '__main__':
    unittest.main()
