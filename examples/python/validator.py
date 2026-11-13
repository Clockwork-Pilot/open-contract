"""Python predicate example contract implementation."""

from __future__ import annotations

from predicate import evaluate_predicate


def validate(value: dict[str, object]) -> bool:
    return evaluate_predicate(value["predicate"], value["json"])
