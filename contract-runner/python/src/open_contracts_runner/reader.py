"""JSON/YAML and consumer supplied contract bundle readers."""

from __future__ import annotations

import json
from collections.abc import Callable, Mapping
import math
from pathlib import Path
from typing import Any, Protocol

import yaml

from .model import Bundle, Case, Target
from .validation import validate_bundle_data


class BundleReader(Protocol):
    def read(self, source: Any) -> Any: ...


class CustomReader(Protocol):
    """Consumer adapter: return the same mapping accepted by normalize_bundle."""

    def read(self, source: Any) -> Any: ...


Reader = BundleReader | CustomReader | Callable[[Any], Any]


class JsonYamlReader:
    def read(self, source: str | Path) -> Any:
        path = Path(source)
        text = path.read_text(encoding="utf-8")
        if path.suffix.lower() == ".json":
            return json.loads(text)
        return yaml.safe_load(text)


def load_bundle(source: Any, reader: Reader | None = None) -> Bundle:
    if isinstance(source, Bundle):
        value = {
            "version": source.version,
            "targets": [
                {
                    "id": target.id,
                    "kind": target.kind,
                    "config": target.config,
                    "cases": [case.to_dict() for case in target.cases],
                }
                for target in source.targets
            ],
        }
    elif isinstance(source, Mapping):
        value = source
    else:
        active_reader = reader or JsonYamlReader()
        value = active_reader(source) if callable(active_reader) else active_reader.read(source)
    validate_bundle_data(value)
    return normalize_bundle(value)


def normalize_bundle(value: Any) -> Bundle:
    value = normalize_json_value(value, "bundle")
    if not isinstance(value, dict):
        raise ValueError("bundle reader must return an object")
    if set(value) - {"version", "kind", "config", "cases", "targets"}:
        raise ValueError("bundle contains unknown fields")
    if type(value.get("version")) is not int or value.get("version") != 1:
        raise ValueError("bundle version must be 1")
    has_targets = "targets" in value
    has_cases = "cases" in value
    if has_targets == has_cases:
        raise ValueError("bundle must contain exactly one of targets or cases")

    raw_targets = value["targets"] if has_targets else [{
        "id": "default",
        "kind": value.get("kind", "custom"),
        "config": value.get("config", {}),
        "cases": value["cases"],
    }]
    if not isinstance(raw_targets, list):
        raise ValueError("targets must be an array")
    targets: list[Target] = []
    target_ids: set[str] = set()
    for raw_target in raw_targets:
        if not isinstance(raw_target, Mapping):
            raise ValueError("each target must be an object")
        if set(raw_target) - {"id", "kind", "config", "cases"}:
            raise ValueError("target contains unknown fields")
        target_id = _required_string(raw_target, "id", "target")
        kind = _required_string(raw_target, "kind", f"target {target_id}")
        if target_id in target_ids:
            raise ValueError(f"duplicate target id: {target_id}")
        target_ids.add(target_id)
        config = raw_target.get("config", {})
        cases_value = raw_target.get("cases")
        if not isinstance(config, Mapping):
            raise ValueError(f"target {target_id} config must be an object")
        if not isinstance(cases_value, list):
            raise ValueError(f"target {target_id} cases must be an array")
        cases: list[Case] = []
        case_ids: set[str] = set()
        for raw_case in cases_value:
            if not isinstance(raw_case, Mapping):
                raise ValueError(f"target {target_id} case must be an object")
            if set(raw_case) - {"id", "input", "expected", "predicate"}:
                raise ValueError(f"case {target_id} contains unknown fields")
            case_id = _required_string(raw_case, "id", f"target {target_id} case")
            if case_id in case_ids:
                raise ValueError(f"duplicate case id {case_id} in target {target_id}")
            case_ids.add(case_id)
            if "input" not in raw_case:
                raise ValueError(f"case {target_id}/{case_id} is missing input")
            has_expected = "expected" in raw_case
            has_predicate = "predicate" in raw_case
            if has_expected == has_predicate:
                raise ValueError(
                    f"case {target_id}/{case_id} must contain exactly one of expected or predicate"
                )
            predicate = raw_case.get("predicate")
            if has_predicate and (not isinstance(predicate, str) or not predicate):
                raise ValueError(f"case {target_id}/{case_id} predicate must be a non-empty string")
            cases.append(Case(case_id, raw_case["input"], raw_case.get("expected"), predicate))
        targets.append(Target(target_id, kind, tuple(cases), dict(config)))
    return Bundle(1, tuple(targets))


def _required_string(value: Mapping[str, Any], key: str, context: str) -> str:
    result = value.get(key)
    if not isinstance(result, str) or not result:
        raise ValueError(f"{context} {key} must be a non-empty string")
    return result


def normalize_json_value(value: Any, context: str = "value") -> Any:
    if value is None or isinstance(value, (str, bool, int)):
        return value
    if isinstance(value, float):
        if math.isfinite(value):
            return value
        raise ValueError(f"{context} contains a non-finite number")
    if isinstance(value, Mapping):
        result = {}
        for key, child in value.items():
            if not isinstance(key, str):
                raise ValueError(f"{context} object keys must be strings")
            result[key] = normalize_json_value(child, f"{context}.{key}")
        return result
    if isinstance(value, list):
        return [normalize_json_value(child, f"{context}[{index}]") for index, child in enumerate(value)]
    raise ValueError(f"{context} contains non-JSON value {type(value).__name__}")
