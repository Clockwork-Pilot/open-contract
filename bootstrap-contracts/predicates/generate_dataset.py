#!/usr/bin/env python3
"""Render the checked-in predicate dataset from its declarative source cases."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parent
RULES = ROOT / "generation-rules.json"
DATASET = ROOT / "dataset.json"


def derive_path(predicate: str) -> str:
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


def render() -> str:
    rules = json.loads(RULES.read_text(encoding="utf-8"))
    cases = rules.get("cases")
    if not isinstance(cases, list):
        raise ValueError("generation-rules.json must contain a cases array")
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
    generated = [
        {**case, "path": derive_path(case["predicate"])}
        for case in cases
    ]
    generated = [
        {key: case[key] for key in ("id", "json", "path", "predicate", "expected")}
        for case in generated
    ]
    return json.dumps({"cases": generated}, indent=2, ensure_ascii=False) + "\n"


if __name__ == "__main__":
    rendered = render()
    if "--check" in __import__("sys").argv:
        if DATASET.read_text(encoding="utf-8") != rendered:
            raise SystemExit("dataset.json is stale; run make generate-dataset")
    else:
        DATASET.write_text(rendered, encoding="utf-8")
