#!/usr/bin/env python3
"""Read-only installed package observations, not an APT solver or runtime qualification."""
import argparse
import json
from pathlib import Path
import re
import subprocess


def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('source_inventory',type=Path);p.add_argument('output',type=Path);a=p.parse_args()
    source=json.loads(a.source_inventory.read_text())
    result=subprocess.run(['/usr/bin/dpkg-query','-W','-f=${Package}\t${Architecture}\t${Version}\t${Status}\n'],check=True,capture_output=True,text=True)
    installed={}
    for line in result.stdout.splitlines():
        name,arch,version,status=line.split('\t',3)
        if status=='install ok installed': installed.setdefault(name,[]).append(dict(architecture=arch,version=version))
    observations=[]
    by_name={p['Package']:p for p in source['packages']}
    for dependency in source['dependencies']:
        if dependency['classification']=='present_required_package':continue
        architecture=by_name[dependency['package']]['Architecture'];alternatives=[]
        for term in dependency['alternatives']:
            match=re.fullmatch(r'([a-z0-9+.-]+)(?::(?:any|native))?(?: \((>=|<=|=|>>|<<) ([a-zA-Z0-9.+:~_-]+)\))?',term)
            if not match:raise ValueError('unsupported dependency expression')
            name,operator,version=match.groups();providers=installed.get(name,[]);matching=[]
            for provider in providers:
                if provider['architecture'] not in (architecture,'all'):continue
                satisfied=not operator or subprocess.run(['/usr/bin/dpkg','--compare-versions',provider['version'],operator,version],check=False).returncode==0
                matching.append(dict(**provider,version_constraint_satisfied=satisfied))
            alternatives.append(dict(expression=term,installed_matching_architecture=matching,installed_other_architectures=[p for p in providers if p['architecture'] not in (architecture,'all')]))
        observations.append(dict(**dependency,architecture=architecture,observations=alternatives,
            status='SATISFIED_BY_OBSERVED_PACKAGE' if any(p['version_constraint_satisfied'] for t in alternatives for p in t['installed_matching_architecture']) else 'NO_MATCHING_INSTALLED_PACKAGE_VIRTUAL_PROVIDER_OR_SOLVER_UNVERIFIED'))
    a.output.write_text(json.dumps(dict(schema_version=1,host_os=Path('/etc/os-release').read_text(),dependencies=observations,runtime_closure='NOT_ESTABLISHED',system_packages_installed=False,authentic_execution='NOT_RUN'),indent=2)+'\n')


if __name__=='__main__':main()
