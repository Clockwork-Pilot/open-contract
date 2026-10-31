"""Validation for normalized datasets against the repository schemas."""

from __future__ import annotations

import json
from pathlib import Path

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

def validate_bundle_data(dataset: object) -> None:
    """Validate normalized dataset data against the bundle and predicate schemas."""
    runner_root = Path(__file__).resolve().parents[2]
    repository_root = runner_root.parents[1]
    bundle_schema = json.loads(
        (runner_root / "schemas" / "bundle.v1.schema.json").read_text(encoding="utf-8")
    )
    predicate_schema = json.loads(
        (
            repository_root
            / "bootstrap-contracts"
            / "predicates"
            / "predicate-language.schema.json"
        ).read_text(encoding="utf-8")
    )
    registry = Registry().with_resources(
        (schema["$id"], Resource.from_contents(schema))
        for schema in (bundle_schema, predicate_schema)
    )
    errors = sorted(
        Draft202012Validator(bundle_schema, registry=registry).iter_errors(dataset),
        key=lambda error: list(error.absolute_path),
    )
    if errors:
        error = errors[0]
        location = "/".join(str(part) for part in error.absolute_path) or "<root>"
        raise ValueError(f"dataset {location}: {error.message}")
