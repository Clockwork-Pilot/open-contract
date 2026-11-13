#!/usr/bin/env python3
"""Run the predicate dataset cases as the Python sandbox."""

from __future__ import annotations

import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent
from predicate import evaluate_predicate  # noqa: E402


def run_dataset(dataset_path: Path = ROOT / "dataset.json") -> int:
    dataset = json.loads(dataset_path.read_text(encoding="utf-8"))
    passed = 0
    failed: list[str] = []
    total = 0
    for case in dataset["cases"]:
        total += 1
        actual = evaluate_predicate(case["input"]["predicate"], case["input"]["json"])
        if actual is case["expected"]:
            passed += 1
        else:
            failed.append(f"cases/{case['id']}: expected {case['expected']!r}, got {actual!r}")
    for failure in failed:
        print(f"FAIL {failure}", file=sys.stderr)
    print(f"Python predicate dataset: {passed}/{total} dataset cases passed")
    return 1 if failed else 0


if __name__ == "__main__":
    dataset_path = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "dataset.json"
    raise SystemExit(run_dataset(dataset_path))
