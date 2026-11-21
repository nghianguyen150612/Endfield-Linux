#!/usr/bin/env python3
"""Explicit bounded WineHQ acquisition; repository integrity is NOT publisher identity.

Never installs packages or executes extracted code. Requires zstd and GPG.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.request

BASE = 'https://dl.winehq.org/wine-builds/ubuntu/'
FINGERPRINT = 'D43F640145369C51D786DDEA76F1A20FF987672F'
VERSION = '10.0.0.0~noble-1'


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args):
        return None


def download(url, path, maximum):
    opener = urllib.request.build_opener(NoRedirect())
    with opener.open(url, timeout=60) as response, path.open('xb') as output:
        if response.geturl() != url:
            raise ValueError('redirect refused')
        count = 0
        while chunk := response.read(1024 * 1024):
            count += len(chunk)
            if count > maximum:
                raise ValueError('download bound exceeded')
            output.write(chunk)


def verify_signed_index(release, data, architecture):
    from wine_package_set import control
    metadata = control(release)
    if metadata.get('Suite') != 'noble' or metadata.get('Codename') != 'noble' or architecture not in metadata.get('Architectures','').split():
        raise ValueError('wrong repository distribution or architecture')
    target = 'main/binary-'+architecture+'/Packages'
    expected = [line.split() for line in metadata['SHA256'].splitlines() if line.endswith(' '+target)]
    if len(expected) != 1 or len(expected[0]) != 3 or len(data) != int(expected[0][1]) or hashlib.sha256(data).hexdigest() != expected[0][0]:
        raise ValueError('index integrity mismatch')


def verify_signer_status(status):
    signatures = [line.split()[2] for line in status.splitlines() if line.startswith('[GNUPG:] VALIDSIG ')]
    if signatures != [FINGERPRINT]:
        raise ValueError('unknown repository signing key')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    root = args.destination
    parent = root.parent.resolve(strict=True)
    st = parent.stat()
    if (os.getuid() == 0 or os.getuid() != os.geteuid() or not root.is_absolute()
            or root.parent != parent or st.st_uid != os.geteuid() or st.st_mode & 0o077):
        parser.error('new absolute destination below private user-owned parent required')
    root.mkdir(mode=0o700)
    download(BASE+'dists/noble/InRelease', root/'InRelease', 100_000)
    download('https://dl.winehq.org/wine-builds/winehq.key', root/'winehq.key', 100_000)
    home = root/'gnupg'; home.mkdir(mode=0o700)
    def gpg(*arguments):
        return subprocess.run(['gpg','--batch','--homedir',str(home),*arguments],
                              capture_output=True, check=True)
    gpg('--import', str(root/'winehq.key'))
    result = gpg('--status-fd=1','--output',str(root/'Release'),'--decrypt',str(root/'InRelease'))
    verify_signer_status(result.stdout.decode())
    indices = {}
    for architecture in ('amd64', 'i386'):
        target = 'main/binary-'+architecture+'/Packages'
        path = root/('Packages-'+architecture)
        download(BASE+'dists/noble/'+target, path, 10_000_000)
        data = path.read_bytes()
        verify_signed_index((root/'Release').read_text(),data,architecture)
        indices[architecture] = data
    records = []
    for name, output, architecture in [('wine-stable-amd64','amd64','amd64'), ('wine-stable-i386','i386-evidence','i386'), ('wine-stable','common','amd64')]:
        matches = []
        for paragraph in indices[architecture].decode().split('\n\n'):
            if paragraph.startswith('Package: '+name+'\n') and '\nVersion: '+VERSION+'\n' in paragraph:
                matches.append(dict(line.split(': ',1) for line in paragraph.splitlines()
                                    if ': ' in line and not line.startswith(' ')))
        if len(matches) != 1:
            raise ValueError('exact package record unavailable or ambiguous')
        record = matches[0]
        if record['Architecture'] != architecture or record.get('Source') != 'wine':
            raise ValueError('unexpected package architecture or source')
        filename = record['Filename']
        if not re.fullmatch(r'pool/main/w/wine/[a-z0-9_.~+-]+\.deb',filename):
            raise ValueError('unexpected package path')
        package = root/(output+'.deb')
        download(BASE+filename,package,min(int(record['Size']),300_000_000))
        if package.stat().st_size != int(record['Size']):
            raise ValueError('package size mismatch')
        subprocess.run([sys.executable,str(Path(__file__).with_name('inspect-wine-deb.py')),
                        str(package),str(root/output),'--expected-sha256',record['SHA256']],check=True)
        records.append(record)
    (root/'records.json').write_text(json.dumps(records,indent=2)+'\n')
    subprocess.run([sys.executable,str(Path(__file__).with_name('prepare-wine-candidate.py')),
                    str(root/'amd64/inventory.json'),str(root/'common/inventory.json'),str(root/'combined'),
                    '--i386',str(root/'i386-evidence/inventory.json')],check=True)
    subprocess.run([sys.executable,str(Path(__file__).with_name('package-wine-candidate.py')),
                    str(root),str(root/'managed'),'--amd64-root',str(root/'amd64/staging'),
                    '--i386-root',str(root/'i386-evidence/staging'),'--common-root',str(root/'common/staging')],check=True)


if __name__ == '__main__':
    main()
