#!/usr/bin/env python3
"""Validate an actual P033 report and reject representative contract violations."""
import copy
import json
import sys
from pathlib import Path
from jsonschema import Draft202012Validator, RefResolver

root = Path(__file__).resolve().parent.parent
schema = json.loads((root / 'docs/schema/dependency-diagnostics-v1.schema.json').read_text())
compatibility = json.loads((root / 'docs/schema/compatibility-diagnostics-v1.schema.json').read_text())
report = json.loads(Path(sys.argv[1]).read_text())
Draft202012Validator.check_schema(schema)
resolver = RefResolver.from_schema(schema, store={compatibility['$id']: compatibility})
validator = Draft202012Validator(schema, resolver=resolver)
validator.validate(report)
for mutate in (
    lambda r: r.pop('schema_version'),
    lambda r: r['storage'].__setitem__('status', 'made_up'),
    lambda r: r['compatibility'].__setitem__('game_compatibility', 'tested'),
    lambda r: r['steam_observation'].__setitem__('unexpected_path', '/private'),
):
    invalid = copy.deepcopy(report)
    mutate(invalid)
    if validator.is_valid(invalid):
        raise SystemExit('Schema accepted an invalid report')
print('P033 report and adversarial schema checks passed')
