"""Case execution and predicate-first matching."""

from __future__ import annotations

from typing import Any

from open_contracts_predicates import evaluate_predicate

from .executors import ExecutorRegistry
from .model import Bundle, Case, CaseResult, RunReport, Target
from .reader import Reader, load_bundle, normalize_json_value


class ContractRunner:
    def __init__(
        self,
        executors: ExecutorRegistry | None = None,
        reader: Reader | None = None,
    ) -> None:
        self.executors = executors or ExecutorRegistry.with_builtins()
        self.reader = reader

    def run(self, source: Any) -> RunReport:
        bundle = load_bundle(source, self.reader)
        results: list[CaseResult] = []
        for target in bundle.targets:
            for case in target.cases:
                results.append(self._run_case(target, case))
        return RunReport(tuple(results))

    def run_bundle(self, bundle: Bundle) -> RunReport:
        return self.run(bundle)

    def _run_case(self, target: Target, case: Case) -> CaseResult:
        try:
            observed = self.executors.get(target.kind).execute(target, case)
            actual = normalize_json_value(observed, "actual")
            passed, matched_by = match_case(case, actual)
            return CaseResult(target.id, case.id, actual, passed, matched_by)
        except Exception as exc:
            return CaseResult(
                target.id,
                case.id,
                error=f"{type(exc).__name__}: {exc}",
            )


def match_case(case: Case, actual: Any) -> tuple[bool, str | None]:
    if case.predicate is not None:
        return (True, "predicate") if evaluate_predicate(case.predicate, actual) else (False, None)
    if _json_equal(actual, case.expected):
        return True, "exact"
    return False, None


def _json_equal(left: Any, right: Any) -> bool:
    if isinstance(left, bool) or isinstance(right, bool):
        return type(left) is type(right) and left == right
    if isinstance(left, (int, float)) and isinstance(right, (int, float)):
        return left == right
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        return left.keys() == right.keys() and all(_json_equal(left[key], right[key]) for key in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(_json_equal(a, b) for a, b in zip(left, right))
    return left == right
