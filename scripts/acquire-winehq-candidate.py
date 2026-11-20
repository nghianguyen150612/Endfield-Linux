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
    signatures = [line.split()[2] for line in result.stdout.decode().splitlines()
                  if line.startswith('[GNUPG:] VALIDSIG ')]
    if signatures != [FINGERPRINT]:
        raise ValueError('unknown repository signing key')
    download(BASE+'dists/noble/main/binary-amd64/Packages', root/'Packages', 10_000_000)
    lines = (root/'Release').read_text().split('SHA256:\n',1)[1].splitlines()
    expected = [line.split() for line in lines if line.endswith(' main/binary-amd64/Packages')]
    data = (root/'Packages').read_bytes()
    if len(expected) != 1 or len(data) != int(expected[0][1]) or hashlib.sha256(data).hexdigest() != expected[0][0]:
        raise ValueError('index integrity mismatch')
    records = []
    for name, output in [('wine-stable-amd64','amd64'), ('wine-stable','common')]:
        matches = []
        for paragraph in data.decode().split('\n\n'):
            if paragraph.startswith('Package: '+name+'\n') and '\nVersion: '+VERSION+'\n' in paragraph:
                matches.append(dict(line.split(': ',1) for line in paragraph.splitlines()
                                    if ': ' in line and not line.startswith(' ')))
        if len(matches) != 1:
            raise ValueError('exact package record unavailable or ambiguous')
        record = matches[0]
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
    (root/'package-records.json').write_text(json.dumps(records,indent=2)+'\n')
    subprocess.run([sys.executable,str(Path(__file__).with_name('prepare-wine-candidate.py')),
                    str(root/'amd64/inventory.json'),str(root/'common/inventory.json'),str(root/'combined')],check=True)


if __name__ == '__main__':
    main()
