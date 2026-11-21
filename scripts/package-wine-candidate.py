#!/usr/bin/env python3
"""Deterministic WineHQ dual-architecture candidate; never signs, installs or runs it."""
import argparse
import gzip
import hashlib
import io
import json
import os
from pathlib import Path
import tarfile
from wine_package_set import combine, control, digest, encode


def package_candidate(records, packages, roots, output):
    combined = combine(records, packages)
    # New private output only; source roots are disposable inspected staging.
    parent = output.parent.resolve(strict=True)
    st = parent.stat()
    if output.parent != parent or st.st_uid != os.geteuid() or st.st_mode & 0o077:
        raise ValueError('private canonical output parent required')
    output.mkdir(mode=0o700)
    by_name = {r['Package']: root for r, root in zip(records, roots)}
    owners = {e['path']: e['packages'] for e in combined['entry_ownership']}
    entries = list(combined['entries'])
    layout = dict(schema_version=1, layout='winehq-dualarch-v1', build='wine-10.0',
        source_inventory_sha256=combined['inventory_sha256'], source_packages=[dict(package=r['Package'],sha256=r['SHA256']) for r in records])
    generated = {'version': b'wine-10.0\n', 'endfield-layout-v1.json': encode(layout)+b'\n'}
    if any(e['path'] in generated for e in entries):
        raise ValueError('generated metadata path collision')
    for path, content in generated.items():
        entries.append(dict(path=path,kind=dict(type='file',size=len(content),sha256=hashlib.sha256(content).hexdigest(),executable=False,setid=False)))
    entries.sort(key=lambda e:e['path'])
    partial = output/'candidate.tar.gz.incomplete'
    with partial.open('xb') as raw, gzip.GzipFile(filename='',fileobj=raw,mode='wb',mtime=0,compresslevel=6) as zipped, tarfile.open(fileobj=zipped,mode='w|',format=tarfile.USTAR_FORMAT) as tar:
        for entry in entries:
            path, kind = entry['path'], entry['kind']
            header = tarfile.TarInfo(path)
            header.uid = header.gid = header.mtime = 0
            header.uname = header.gname = ''
            if kind['type'] == 'directory':
                header.type = tarfile.DIRTYPE; header.mode = 0o700
                tar.addfile(header)
            elif kind['type'] == 'symlink':
                header.type = tarfile.SYMTYPE; header.linkname = kind['target']; header.mode = 0o777
                tar.addfile(header)
            else:
                header.mode = 0o700 if kind['executable'] else 0o600
                if path in generated:
                    data = generated[path]
                else:
                    source = by_name[owners[path][0]]/path
                    # Refuse symlink parents and changed staged bytes.
                    cursor = source
                    while cursor != by_name[owners[path][0]]:
                        if cursor.is_symlink(): raise ValueError('symlink staging path')
                        cursor = cursor.parent
                    with source.open('rb') as f:
                        data = f.read(100_000_001)
                if len(data) != kind['size'] or hashlib.sha256(data).hexdigest() != kind['sha256']:
                    raise ValueError('staging bytes changed')
                header.size = len(data); tar.addfile(header,io.BytesIO(data))
    with partial.open('rb') as source:
        artifact_sha = hashlib.file_digest(source,'sha256').hexdigest()
    final = output/'winehq-noble-10.0-dualarch-candidate.tar.gz'
    partial.rename(final)
    report = dict(schema_version=1,source_inventory_sha256=combined['inventory_sha256'],
        source_evidence_sha256=hashlib.sha256((json.dumps(combined,indent=2)+'\n').encode()).hexdigest(),
        transformed_inventory_sha256=digest(entries),source_packages=layout['source_packages'],
        artifact_sha256=artifact_sha,artifact_size=final.stat().st_size, generation=1,
        generation_identity=digest(dict(artifact_sha256=artifact_sha,source_inventory_sha256=combined['inventory_sha256'],generation=1)),
        layout='winehq-dualarch-v1',generated_metadata=sorted(generated),
        transformations=['retain original payload paths and exact file bytes','normalize uid/gid and mtime to zero','normalize directories to 0700 and files to 0700/0600 as P025 extracts','retain safe source symlink targets','add local version and layout metadata'],
        upstream_package_integrity='VERIFIED_FOR_REPOSITORY_KEY',transformation_integrity='LOCALLY_COMPUTED',
        manifest_signer='NONE',p024='BLOCKED_NO_ENROLLED_MANIFEST_AUTHORITY',p025='STRUCTURAL_CANDIDATE_ONLY',p027='NOT_INSTALLED_OR_PINNED',
        authentic_execution='NOT_RUN',production_initialization='BLOCKED',game_launch='BLOCKED')
    for name, value in [('source-inventory.json',combined),('transformed-inventory.json',entries),('packaging.json',report)]:
        (output/name).write_text(json.dumps(value,indent=2,sort_keys=(name=='packaging.json'))+'\n')
    return report


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('evidence',type=Path);p.add_argument('output',type=Path)
    p.add_argument('--amd64-root',required=True,type=Path);p.add_argument('--i386-root',required=True,type=Path);p.add_argument('--common-root',required=True,type=Path)
    a=p.parse_args()
    packages=[json.loads((a.evidence/n/'inventory.json').read_text()) for n in ['amd64','i386-evidence','common']]
    records=json.loads((a.evidence/'records.json').read_text())
    print(json.dumps(package_candidate(records,packages,[a.amd64_root,a.i386_root,a.common_root],a.output),indent=2))


if __name__=='__main__': main()
