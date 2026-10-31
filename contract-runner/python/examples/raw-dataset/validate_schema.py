"""Validate an example dataset against the published bundle schemas."""

from __future__ import annotations

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource


ROOT = Path(__file__).resolve().parents[2]
PREDICATE_SCHEMA_PATH = ROOT.parents[1] / "bootstrap-contracts/predicates/predicate-language.schema.json"
BUNDLE_SCHEMA_PATH = ROOT / "schemas/bundle.v1.schema.json"


def main() -> int:
    dataset_path = Path(sys.argv[1])
    bundle_schema = json.loads(BUNDLE_SCHEMA_PATH.read_text(encoding="utf-8"))
    predicate_schema = json.loads(PREDICATE_SCHEMA_PATH.read_text(encoding="utf-8"))
    registry = Registry().with_resources(
        (schema["$id"], Resource.from_contents(schema))
        for schema in (bundle_schema, predicate_schema)
    )
    dataset = json.loads(dataset_path.read_text(encoding="utf-8"))
    Draft202012Validator(bundle_schema, registry=registry).validate(dataset)
    print(f"Bundle schema valid: {dataset_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
