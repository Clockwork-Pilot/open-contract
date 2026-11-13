"""Typed normalized model and serializable execution reports."""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class Case:
    id: str
    input: Any
    expected: Any = None
    predicate: str | None = None
    steps: tuple[dict[str, Any], ...] = ()
    context: dict[str, Any] | None = None
    expected_is_set: bool | None = None

    @property
    def has_expected(self) -> bool:
        if self.expected_is_set is not None:
            return self.expected_is_set
        return self.predicate is None or self.expected is not None

@dataclass(frozen=True)
class Target:
    id: str
    kind: str
    cases: tuple[Case, ...]
    config: dict[str, Any] = field(default_factory=dict)


@dataclass(frozen=True)
class Bundle:
    version: int
    targets: tuple[Target, ...]


@dataclass(frozen=True)
class CaseResult:
    target_id: str
    case_id: str
    actual: Any = None
    passed: bool = False
    matched_by: str | None = None
    error: str | None = None

    def to_dict(self) -> dict[str, Any]:
        return {
            "target_id": self.target_id,
            "case_id": self.case_id,
            "actual": self.actual,
            "passed": self.passed,
            "matched_by": self.matched_by,
            "error": self.error,
        }


@dataclass(frozen=True)
class RunReport:
    results: tuple[CaseResult, ...]

    @property
    def passed(self) -> int:
        return sum(result.passed for result in self.results)

    @property
    def total(self) -> int:
        return len(self.results)

    @property
    def by_target(self) -> dict[str, tuple[CaseResult, ...]]:
        grouped: dict[str, list[CaseResult]] = {}
        for result in self.results:
            grouped.setdefault(result.target_id, []).append(result)
        return {target: tuple(items) for target, items in grouped.items()}

    def to_dict(self) -> dict[str, Any]:
        return {
            "passed": self.passed,
            "total": self.total,
            "targets": {
                target: {
                    "passed": sum(result.passed for result in results),
                    "total": len(results),
                    "cases": [result.to_dict() for result in results],
                }
                for target, results in self.by_target.items()
            },
        }
