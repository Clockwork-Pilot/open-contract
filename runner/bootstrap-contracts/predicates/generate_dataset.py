#!/usr/bin/env python3
"""Generate predicate cases from the language schema and authored edge cases."""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parent
DATASET = ROOT / "dataset.json"
LANGUAGE_SCHEMA = ROOT / "predicate-language.schema.json"
DATASET_CASES = ROOT / "dataset-cases.json"


def derive_path(predicate: str) -> str:
    """Derive the first referenced field as JSON Pointer metadata."""
    field = re.search(
        r'(?:\.[A-Za-z0-9_@-]+|\["(?:\\.|[^"\\])*"\])+',
        predicate,
    )
    if field is None:
        return ""
    text = field.group(0)
    parts: list[str] = []
    index = 0
    while index < len(text):
        if text[index] == ".":
            index += 1
            start = index
            while index < len(text) and text[index] not in ".[":
                index += 1
            parts.append(text[start:index])
        else:
            end = text.index("]", index + 1)
            segment = text[index + 1 : end]
            parts.append(json.loads(segment) if segment.startswith('"') else segment)
            index = end + 1
    return "/" + "/".join(
        part.replace("~", "~0").replace("/", "~1") for part in parts
    )


def _sample_record(sample: dict[str, Any], field: str) -> dict[str, Any]:
    return {field: sample["value"]} if sample["present"] else {}


def _sample_value(sample: dict[str, Any]) -> Any:
    return sample["value"] if sample["present"] else None


def _strict_equal(
    left_category: str,
    left_sample: dict[str, Any],
    right_category: str,
    right_sample: dict[str, Any],
    comparison_rules: dict[str, Any],
) -> bool:
    left = _sample_value(left_sample)
    right = _sample_value(right_sample)
    missing_category = comparison_rules["missingEquivalentCategory"]
    if left_category == "missing":
        left_category, left = missing_category, None
    if right_category == "missing":
        right_category, right = missing_category, None
    if isinstance(left, bool) != isinstance(right, bool):
        return False
    if left_category == right_category == comparison_rules["numericCategory"]:
        return left == right
    return left_category == right_category and type(left) is type(right) and left == right


def _ordered_value(value: Any, category: str) -> Any | None:
    if category == "boolean" and isinstance(value, bool):
        return value
    if category == "number" and isinstance(value, (int, float)) and not isinstance(value, bool):
        return value
    if category == "string" and isinstance(value, str):
        return value
    return None


def _expected_exists(sample: dict[str, Any], function: dict[str, Any]) -> bool:
    if not sample["present"]:
        category = "missing"
    else:
        value = sample["value"]
        if value is None:
            category = "null"
        elif value == {}:
            category = "emptyObject"
        elif value == []:
            category = "emptyArray"
        else:
            category = function["trueFor"]
    return category not in function["falseFor"]


def _expected_comparison(
    left_type: dict[str, Any],
    left_sample: dict[str, Any],
    operator: str,
    right_type: dict[str, Any],
    right_sample: dict[str, Any],
    comparison_rules: dict[str, Any],
) -> bool:
    left_category = left_type["category"]
    right_category = right_type["category"]
    equal = _strict_equal(
        left_category, left_sample, right_category, right_sample, comparison_rules
    )
    if operator == "==":
        return equal
    if operator == "!=":
        return not equal
    ordering_categories = comparison_rules["orderingCategories"]
    if left_category not in ordering_categories or right_category != left_category:
        return comparison_rules["unsupportedOrderingResult"]
    left = _ordered_value(_sample_value(left_sample), left_category)
    right = _ordered_value(_sample_value(right_sample), right_category)
    if left is None or right is None:
        return comparison_rules["unsupportedOrderingResult"]
    return {
        ">": left > right,
        ">=": left >= right,
        "<": left < right,
        "<=": left <= right,
    }[operator]


def generate_language_cases(language: dict[str, Any]) -> list[dict[str, Any]]:
    """Make type-pair comparisons and exists() cases from schema samples."""
    data_types = language.get("dataTypes")
    comparison_ops = language.get("operators", {}).get("comparison")
    if not isinstance(data_types, list) or not isinstance(comparison_ops, list):
        raise ValueError("predicate schema must declare dataTypes and comparison operators")
    comparison_rules = language["comparisonRules"]
    exists_function = next(
        function
        for function in language["operators"]["functions"]
        if function["name"] == "exists"
    )

    generated = []
    for left_type in data_types:
        left_sample = left_type["comparisonSample"]
        for right_type in data_types:
            right_sample = right_type["comparisonSample"]
            for operation in comparison_ops:
                operator = operation["token"]
                slug = {
                    "==": "equal",
                    "!=": "not-equal",
                    ">": "greater",
                    ">=": "greater-equal",
                    "<": "less",
                    "<=": "less-equal",
                }[operator]
                generated.append(
                    {
                        "id": f"type-{left_type['id']}-{slug}-{right_type['id']}",
                        "json": {
                            **_sample_record(left_sample, "left"),
                            **_sample_record(right_sample, "right"),
                        },
                        "predicate": f".left {operator} .right",
                        "expected": _expected_comparison(
                            left_type,
                            left_sample,
                            operator,
                            right_type,
                            right_sample,
                            comparison_rules,
                        ),
                    }
                )

        for index, sample in enumerate(left_type.get("existenceSamples", []), start=1):
            generated.append(
                {
                    "id": f"type-exists-{left_type['id']}-{index}",
                    "json": _sample_record(sample, "value"),
                    "predicate": "exists(.value)",
                        "expected": _expected_exists(sample, exists_function),
                }
            )
    return generated


def render() -> str:
    schema = json.loads(LANGUAGE_SCHEMA.read_text(encoding="utf-8"))
    language = schema["x-open-contracts-language"]
    case_source = json.loads(DATASET_CASES.read_text(encoding="utf-8"))
    authored_cases = case_source.get("cases")
    if not isinstance(authored_cases, list):
        raise ValueError("dataset-cases.json must contain a cases array")
    cases = authored_cases + generate_language_cases(language)

    ids: set[str] = set()
    for case in cases:
        if set(case) != {"id", "json", "predicate", "expected"}:
            raise ValueError(f"case has unexpected shape: {case.get('id', '<unknown>')}")
        if not isinstance(case["id"], str) or case["id"] in ids:
            raise ValueError(f"case id is missing or duplicated: {case.get('id')!r}")
        ids.add(case["id"])
        if not isinstance(case["predicate"], str):
            raise ValueError(f"predicate must be a string: {case['id']}")
        if not isinstance(case["expected"], bool):
            raise ValueError(f"expected must be a boolean: {case['id']}")

    generated = [{**case, "path": derive_path(case["predicate"])} for case in cases]
    generated = [
        {key: case[key] for key in ("id", "json", "path", "predicate", "expected")}
        for case in generated
    ]
    return json.dumps({"cases": generated}, indent=2, ensure_ascii=False) + "\n"


if __name__ == "__main__":
    rendered = render()
    if "--check" in sys.argv:
        if DATASET.read_text(encoding="utf-8") != rendered:
            raise SystemExit("dataset.json is stale; run make generate-dataset")
    else:
        DATASET.write_text(rendered, encoding="utf-8")
