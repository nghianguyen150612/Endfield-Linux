#!/usr/bin/env python3
"""Offline package closure, provenance and deterministic transformation tests."""
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest
from wine_package_set import combine,digest
spec=importlib.util.spec_from_file_location('pack',Path(__file__).with_name('package-wine-candidate.py'))
pack=importlib.util.module_from_spec(spec);spec.loader.exec_module(pack)


def fixture():
    names=['wine-stable-amd64','wine-stable-i386','wine-stable']
    records=[];packages=[]
    for i,name in enumerate(names):
        record=dict(Package=name,Architecture='i386' if i==1 else 'amd64',Version='10.0.0.0~noble-1',SHA256=str(i)*64)
        if i==2: record['Depends']='wine-stable-amd64 (= 10.0.0.0~noble-1), wine-stable-i386 (= 10.0.0.0~noble-1), libc6 (>= 2.38)'
        entries=[dict(path='file'+str(i),kind=dict(type='file',size=1,sha256=__import__('hashlib').sha256(b'x').hexdigest(),executable=False,setid=False))]
        package=dict(package_sha256=record['SHA256'],entries=entries,inventory_sha256=digest(entries),observations=[dict(path=entries[0]['path'],archive_path='./'+entries[0]['path'],mode=0o644,uid=0,gid=0,uname='root',gname='root')],package_control='\n'.join(k+': '+v for k,v in record.items() if k!='SHA256'))
        records.append(record);packages.append(package)
    return records,packages

acquire_spec=importlib.util.spec_from_file_location('acquire',Path(__file__).with_name('acquire-winehq-candidate.py'))
acquire=importlib.util.module_from_spec(acquire_spec);acquire_spec.loader.exec_module(acquire)


class PackageSetTests(unittest.TestCase):
    def test_index_digest_distribution_and_unknown_signer(self):
        import hashlib
        data=b'fixture'
        release='Suite: noble\nCodename: noble\nArchitectures: amd64 i386\nSHA256:\n '+hashlib.sha256(data).hexdigest()+' 7 main/binary-i386/Packages\n'
        acquire.verify_signed_index(release,data,'i386')
        for text,payload in [(release,b'changed'),(release.replace('noble','jammy'),data)]:
            with self.assertRaises(ValueError):acquire.verify_signed_index(text,payload,'i386')
        with self.assertRaises(ValueError):acquire.verify_signer_status('[GNUPG:] VALIDSIG '+'0'*40)

    def test_valid_graph_and_missing_companion(self):
        records,packages=fixture();result=combine(records,packages)
        self.assertEqual(result['package_set'],'COMPLETE_THREE_WINE_PACKAGES')
        self.assertEqual(result['runtime_dependency_closure'],'NOT_ESTABLISHED')
        with self.assertRaises(ValueError):combine(records[:2],packages[:2])
        self.assertTrue(any(d['classification']=='system_required' for d in result['dependencies']))

    def test_wrong_version_arch_digest_control_and_ownership(self):
        for change in range(6):
            records,packages=fixture()
            if change==0:records[1]['Version']='11.0'
            elif change==1:records[1]['Architecture']='amd64'
            elif change==2:records[1]['SHA256']='f'*64
            elif change==3:packages[1]['package_control']+='\nVersion: forged'
            elif change==4:packages[1]['observations'][0]['uid']=1000
            else:packages[1]['entries'].pop()
            with self.subTest(change=change),self.assertRaises(ValueError):combine(records,packages)

    def test_conflict_duplicate_file_ownership_and_unsafe_paths(self):
        for path in ['file0','../escape','/absolute']:
            records,packages=fixture();packages[1]['entries'][0]['path']=path;packages[1]['observations'][0]['path']=path;packages[1]['inventory_sha256']=digest(packages[1]['entries'])
            with self.subTest(path=path),self.assertRaises(ValueError):combine(records,packages)

    def test_generated_metadata_cannot_override_payload(self):
        records,packages=fixture()
        packages[0]['entries'][0]['path']='version';packages[0]['observations'][0]['path']='version'
        packages[0]['inventory_sha256']=digest(packages[0]['entries'])
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp);root.chmod(0o700)
            with self.assertRaises(ValueError):pack.package_candidate(records,packages,[root]*3,root/'candidate')
            self.assertFalse((root/'candidate/packaging.json').exists())

    def test_cross_package_non_directory_ancestors_rejected_before_output(self):
        for ancestor_type in ['file', 'symlink']:
            for reverse in [False, True]:
                records,packages=fixture()
                packages[0]['entries'][0]['path']='shared'
                packages[0]['observations'][0]['path']='shared'
                if ancestor_type=='symlink':
                    packages[0]['entries'][0]['kind']=dict(type='symlink',target='elsewhere')
                packages[1]['entries'][0]['path']='shared/nested/child'
                packages[1]['observations'][0]['path']='shared/nested/child'
                for package in packages:package['inventory_sha256']=digest(package['entries'])
                if reverse:records.reverse();packages.reverse()
                with tempfile.TemporaryDirectory() as temp:
                    root=Path(temp);root.chmod(0o700)
                    with self.subTest(ancestor=ancestor_type,reverse=reverse),self.assertRaisesRegex(ValueError,'non-directory combined ancestor'):
                        pack.package_candidate(records,packages,[root]*3,root/'candidate')
                    self.assertFalse((root/'candidate').exists())

    def test_shared_multiarch_directories_remain_valid(self):
        records,packages=fixture()
        for index,package in enumerate(packages):
            package['entries'][0]['path']='shared/arch'+str(index)+'/file'
            package['observations'][0]['path']=package['entries'][0]['path']
            package['entries'].append(dict(path='shared',kind=dict(type='directory',mode=0o700)))
            package['observations'].append(dict(path='shared',archive_path='./shared',mode=0o755,uid=0,gid=0,uname='root',gname='root'))
            package['inventory_sha256']=digest(package['entries'])
        result=combine(records,packages)
        shared=next(e for e in result['entry_ownership'] if e['path']=='shared')
        self.assertEqual(len(shared['packages']),3)

    def test_deterministic_original_vs_derived_and_changed_staging(self):
        records,packages=fixture()
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);root.chmod(0o700);roots=[]
            for i in range(3):
                staging=root/str(i);staging.mkdir();(staging/('file'+str(i))).write_bytes(b'x');roots.append(staging)
            a=pack.package_candidate(records,packages,roots,root/'a');b=pack.package_candidate(records,packages,roots,root/'b')
            self.assertEqual(a,b);self.assertEqual((root/'a/winehq-noble-10.0-dualarch-candidate.tar.gz').read_bytes(),(root/'b/winehq-noble-10.0-dualarch-candidate.tar.gz').read_bytes())
            self.assertNotIn(a['artifact_sha256'],[r['SHA256'] for r in records]);self.assertEqual(a['manifest_signer'],'NONE')
            (roots[0]/'file0').write_bytes(b'y')
            with self.assertRaises(ValueError):pack.package_candidate(records,packages,roots,root/'changed')


if __name__=='__main__':unittest.main()
