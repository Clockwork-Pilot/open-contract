#!/usr/bin/env python3
"""Require complete executable-statement coverage from dataset data alone."""

from __future__ import annotations

import ast
import sys
import trace
from pathlib import Path

from run_dataset import ROOT, run_dataset

SOURCE = (Path(__file__).parent / "src/open_contracts_predicates/evaluate.py").resolve()


def executable_statement_lines() -> set[int]:
    tree = ast.parse(SOURCE.read_text(encoding="utf-8"))
    lines: set[int] = set()
    for function in (node for node in ast.walk(tree) if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))):
        body = function.body
        if (
            body
            and isinstance(body[0], ast.Expr)
            and isinstance(body[0].value, ast.Constant)
            and isinstance(body[0].value.value, str)
        ):
            body = body[1:]
        for statement in body:
            for node in ast.walk(statement):
                if isinstance(node, ast.stmt) and not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                    lines.add(node.lineno)
    return lines


def main() -> int:
    tracer = trace.Trace(count=True, trace=False)
    result = tracer.runfunc(run_dataset, ROOT / "dataset.json")
    if result:
        return result
    counts = tracer.results().counts
    required = executable_statement_lines()
    covered = {
        line
        for (filename, line), count in counts.items()
        if count and Path(filename).resolve() == SOURCE
    }
    missing = sorted(required - covered)
    print(f"Python predicate evaluator: {len(required) - len(missing)}/{len(required)} executable lines")
    if missing:
        print(f"uncovered evaluator lines: {missing}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
