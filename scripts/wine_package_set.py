"""Pure, non-authorizing multi-package closure and normalized inventory checks."""
import hashlib
import json
import re

EXPECTED = {'wine-stable-amd64': 'amd64', 'wine-stable-i386': 'i386', 'wine-stable': 'amd64'}
VERSION = '10.0.0.0~noble-1'

def safe_path(path):
    return isinstance(path,str) and 0 < len(path) <= 4096 and not any(ord(c)<32 or ord(c)==127 for c in path) and '\\' not in path and all(p not in ('', '.', '..') for p in path.split('/'))


def encode(value):
    return json.dumps(value, separators=(',', ':'), ensure_ascii=False).encode()


def digest(value):
    return hashlib.sha256(encode(value)).hexdigest()


def control(text):
    result = {}
    key = None
    for line in text.splitlines():
        if line.startswith((' ', '\t')) and key:
            result[key] += '\n'+line
        elif line:
            key, value = line.split(':', 1)
            if key in result:
                raise ValueError('duplicate control field')
            result[key] = value.strip()
    return result


def combine(records, packages):
    if len(records) != 3 or len(packages) != 3:
        raise ValueError('missing required package')
    found, merged, ownership, observations, graph = {}, {}, {}, {}, []
    for record, package in zip(records, packages):
        name = record['Package']
        if name not in EXPECTED or name in found or record['Architecture'] != EXPECTED[name] or record['Version'] != VERSION:
            raise ValueError('unexpected package identity')
        if record['SHA256'] != package['package_sha256'] or digest(package['entries']) != package['inventory_sha256']:
            raise ValueError('package/inventory integrity mismatch')
        metadata = control(package['package_control'])
        for key in ('Package', 'Architecture', 'Version', 'Depends', 'Pre-Depends', 'Recommends'):
            if metadata.get(key) != record.get(key):
                raise ValueError('control/index identity or dependency mismatch: '+key)
        found[name] = record
        obs = {o['path']: o for o in package['observations']}
        if len(obs) != len(package['entries']) or len(obs) != len(package['observations']):
            raise ValueError('incomplete mode/ownership observations')
        seen = set()
        for entry in package['entries']:
            path = entry['path']
            if not safe_path(path) or (entry['kind']['type']=='symlink' and not safe_path(entry['kind']['target'])):
                raise ValueError('unsafe path')
            if path in seen or path not in obs or obs[path]['uid'] != 0 or obs[path]['gid'] != 0 or obs[path]['mode'] & 0o6000:
                raise ValueError('duplicate or unsupported ownership')
            seen.add(path)
            kind = entry['kind']
            if kind['type'] not in ('file','directory','symlink'):
                raise ValueError('unsupported file type')
            if kind['type']=='file' and (kind.get('setid') is not False or kind['executable'] != bool(obs[path]['mode'] & 0o111) or not 0 <= kind['size'] <= 100_000_000 or not re.fullmatch('[a-f0-9]{64}',kind['sha256'])):
                raise ValueError('invalid file identity or mode observation')
            previous = merged.get(path)
            if previous and (entry['kind']['type'] != 'directory' or entry != previous):
                raise ValueError('conflicting path or duplicate file ownership: '+path)
            merged[path] = entry
            ownership.setdefault(path, []).append(name)
            observations.setdefault(path, []).append(dict(package=name, **obs[path]))
        for field, category in [('Depends','system_required'),('Pre-Depends','system_required'),('Recommends','optional_recommended')]:
            for group in metadata.get(field, '').split(','):
                if not group.strip():
                    continue
                alternatives = [x.strip() for x in group.split('|')]
                internal = []
                for alternative in alternatives:
                    match = re.fullmatch(r'(wine-stable(?:-amd64|-i386)?) \(= ([^)]+)\)', alternative)
                    if match:
                        if match[2] != VERSION:
                            raise ValueError('wrong companion dependency version')
                        internal.append(match[1])
                graph.append(dict(package=name, field=field, alternatives=alternatives,
                    classification='present_required_package' if internal else category,
                    required_packages=internal, host_availability='NOT_ESTABLISHED' if not internal else 'NOT_APPLICABLE'))
    if set(found) != set(EXPECTED) or any(n not in found for g in graph for n in g['required_packages']):
        raise ValueError('missing required companion')
    # Individually safe DEBs can introduce unsafe ancestors when combined.
    # Validate the complete namespace, independently of package/entry order,
    # before packaging can materialize any links or regular files.
    for path in merged:
        parts = path.split('/')
        for length in range(1, len(parts)):
            ancestor = merged.get('/'.join(parts[:length]))
            if ancestor and ancestor['kind']['type'] != 'directory':
                raise ValueError('non-directory combined ancestor: '+ancestor['path'])
    entries = sorted(merged.values(), key=lambda e: e['path'])
    return dict(schema_version=1, packages=records, entries=entries,
        inventory_sha256=digest(entries), entry_ownership=[dict(path=p, packages=sorted(ownership[p]), observations=sorted(observations[p],key=lambda o:o['package'])) for p in sorted(merged)],
        dependencies=graph, package_set='COMPLETE_THREE_WINE_PACKAGES', runtime_dependency_closure='NOT_ESTABLISHED')
