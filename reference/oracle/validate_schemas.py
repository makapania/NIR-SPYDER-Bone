"""Validate the engine-check parameter files and analysis profiles against their JSON Schemas (developer tool; needs
jsonschema). Run:  uv run --no-project --with jsonschema python reference/oracle/validate_schemas.py plugins
"""
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

root = Path(sys.argv[1] if len(sys.argv) > 1 else "plugins")
bad = 0
for p in sorted(list((root / "checks").glob("check.*.json")) + list((root / "profiles").glob("profile.*.json"))):
    doc = json.loads(p.read_text(encoding="utf-8"))
    sname = (f"engine_check.{doc['check']}.schema.json" if doc.get("format") == "spyder-bone/engine_check"
             else "analysis_profile.schema.json")
    schema = json.loads((root / "schemas" / sname).read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    errs = list(Draft202012Validator(schema).iter_errors(doc))
    bad += len(errs)
    print(f"{p.name}: {'schema OK' if not errs else f'{len(errs)} errors'}")
    for e in errs[:5]:
        print("   ", list(e.absolute_path), e.message[:200])
sys.exit(1 if bad else 0)
