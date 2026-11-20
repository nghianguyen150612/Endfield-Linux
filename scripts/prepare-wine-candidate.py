#!/usr/bin/env python3
"""Combine inspected package evidence into a non-authorizing candidate profile.

No managed installation, executable transformation, signing, or execution.
"""
import argparse
import hashlib
import json
from pathlib import Path


def encoded(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('amd64', type=Path)
    parser.add_argument('common', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    packages = [json.loads(p.read_text()) for p in (args.amd64, args.common)]
    merged = {}
    for package in packages:
        for entry in package['entries']:
            old = merged.get(entry['path'])
            if old and old != entry:
                raise ValueError('conflicting package entries')
            merged[entry['path']] = entry
    entries = sorted(merged.values(), key=lambda e: e['path'])
    roles = []
    prefix = 'opt/wine-stable/'
    def role(name, path, kind):
        entry = merged[prefix + path]
        if entry['kind']['type'] != 'file':
            raise ValueError('role must be regular file')
        roles.append(dict(role=name, path=entry['path'], sha256=entry['kind']['sha256'], kind=kind))
    role('launcher', 'bin/wine64', 'native_executable')
    role('preloader', 'bin/wine64-preloader', 'native_executable')
    role('wineserver', 'bin/wineserver', 'native_executable')
    role('ntdll_unix', 'lib64/wine/x86_64-unix/ntdll.so', 'unix_module')
    names = ['ntdll', 'wineboot', 'kernel32', 'kernelbase', 'services', 'rundll32', 'setupapi', 'advapi32', 'shell32', 'ws2_32', 'newdev']
    for name in names:
        role(name, 'lib64/wine/x86_64-windows/'+name+('.exe' if name in ('wineboot','services','rundll32') else '.dll'), 'windows_module')
    role('wine_inf', 'share/wine/wine.inf', 'resource')
    roles.sort(key=lambda r: r['role'])
    profile = dict(schema_version=2, source_version='10.0',
        source_commit='b073859675060c9211fcbccfd90e4e87520dc2c2',
        source_relationship='upstream_reference_not_binary_attestation',
        artifact_build='winehq-noble-10.0.0.0~noble-1-amd64-and-common', architecture='x86_64',
        build_configuration='observed-classic-amd64-plus-i386-package-dependency-configure-arguments-unavailable',
        installed_prefix='/opt/wine-stable', launcher_role='launcher', launcher_path=prefix+'bin/wine64',
        preloader_policy='required_pathname_handoff_unverified', wineserver_path=prefix+'bin/wineserver',
        unix_module_directories=[prefix+'lib64/wine/x86_64-unix'],
        windows_module_directories=[prefix+'lib64/wine/x86_64-windows'],
        required_initialization_roles=['launcher','ntdll_unix']+names,
        required_resource_roles=['wine_inf'], native_helper_roles=['preloader','wineserver'],
        invocation_semantics='descriptor_then_upstream_pathname_reexec', compiler_provenance='NOT_ESTABLISHED',
        required_host_abi='ELF64-LE-x86_64; package libc6 >= 2.38; static closure separately inspected',
        package_sha256=packages[0]['package_sha256'], inventory_sha256=hashlib.sha256(encoded(entries)).hexdigest(),
        provenance='repository_integrity_publisher_identity_unverified', roles=roles,
        missing_companion_packages=['wine-stable-i386 (= 10.0.0.0~noble-1)'])
    args.output.mkdir(mode=0o700)
    (args.output/'profile.json').write_text(json.dumps(profile,indent=2)+'\n')
    (args.output/'inventory.json').write_text(json.dumps(entries,indent=2)+'\n')
    print(json.dumps({'entries':len(entries),'inventory_sha256':profile['inventory_sha256'],
                      'profile_sha256':hashlib.sha256((args.output/'profile.json').read_bytes()).hexdigest()}))


if __name__ == '__main__':
    main()
