#!/usr/bin/env python3
"""Offline malicious DEB payload tests. Never downloads or runs Wine."""
import importlib.util
import io
from pathlib import Path
import tarfile
import unittest

spec = importlib.util.spec_from_file_location('deb', Path(__file__).with_name('inspect-wine-deb.py'))
deb = importlib.util.module_from_spec(spec)
spec.loader.exec_module(deb)


def archive(items):
    output = io.BytesIO()
    with tarfile.open(fileobj=output, mode='w:gz', format=tarfile.USTAR_FORMAT) as tar:
        for name, kind, target in items:
            entry = tarfile.TarInfo(name)
            entry.type = kind
            entry.linkname = target
            tar.addfile(entry)
    return output.getvalue()


class DebTests(unittest.TestCase):
    def test_unsafe_paths_and_types(self):
        for name, kind, target in [('/escape', tarfile.REGTYPE, ''),
                                   ('../escape', tarfile.REGTYPE, ''),
                                   ('a/../escape', tarfile.REGTYPE, ''),
                                   ('a', tarfile.SYMTYPE, '../../escape'),
                                   ('a', tarfile.LNKTYPE, 'b'),
                                   ('a', tarfile.CHRTYPE, ''),
                                   ('a', tarfile.FIFOTYPE, '')]:
            with self.subTest(name=name, kind=kind), self.assertRaises(ValueError):
                deb.inventory(archive([(name, kind, target)]))

    def test_duplicate_and_parent_conflict(self):
        for names in [('a','a'), ('a','a/b'), ('a/b','a')]:
            with self.subTest(names=names), self.assertRaises(ValueError):
                deb.inventory(archive([(n,tarfile.REGTYPE,'') for n in names]))

    def test_symlink_is_evidence_only_and_cannot_be_parent(self):
        data = archive([('dir/link',tarfile.SYMTYPE,'file')])
        self.assertEqual(deb.inventory(data)[0]['kind'], {'type':'symlink','target':'file'})
        with self.assertRaises(ValueError):
            deb.inventory(archive([('a',tarfile.SYMTYPE,'dir'),('a/b',tarfile.REGTYPE,'')]))

    def test_ar_bad_magic_truncation_and_unknown_member(self):
        for data in [b'', b'!<arch>\nshort', b'!<arch>\n'+b'bad'.ljust(58)+b'`\n']:
            with self.subTest(data=data), self.assertRaises(ValueError):
                deb.ar_members(data)

    def test_inventory_is_sorted_and_deterministic(self):
        data = archive([('b',tarfile.REGTYPE,''),('a',tarfile.REGTYPE,'')])
        self.assertEqual([e['path'] for e in deb.inventory(data)], ['a','b'])
        self.assertEqual(deb.inventory(data), deb.inventory(data))


if __name__ == '__main__':
    unittest.main()
