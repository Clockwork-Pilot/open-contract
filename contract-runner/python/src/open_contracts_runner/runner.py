"""Case execution, predicate-first matching, and compound coverage."""

from __future__ import annotations

from typing import Any

from open_contracts_predicates import evaluate_predicate

from .executors import ExecutorRegistry
from .model import Bundle, Case, CaseResult, RunReport, Target
from .reader import Reader, load_bundle, normalize_json_value


class CoverageThresholdError(RuntimeError):
    def __init__(self, report: RunReport, threshold: float) -> None:
        self.report = report
        self.threshold = threshold
        super().__init__(
            f"contract coverage {report.coverage:.2f}% is below {threshold:.2f}% "
            f"({report.passed}/{report.total} cases passed)"
        )


class ContractRunner:
    def __init__(
        self,
        executors: ExecutorRegistry | None = None,
        reader: Reader | None = None,
    ) -> None:
        self.executors = executors or ExecutorRegistry.with_builtins()
        self.reader = reader

    def run(self, source: Any, *, coverage_threshold: float | None = None) -> RunReport:
        bundle = load_bundle(source, self.reader)
        results: list[CaseResult] = []
        for target in bundle.targets:
            for case in target.cases:
                results.append(self._run_case(target, case))
        report = RunReport(tuple(results))
        if coverage_threshold is not None:
            _enforce_threshold(report, coverage_threshold)
        return report

    def run_bundle(self, bundle: Bundle, *, coverage_threshold: float | None = None) -> RunReport:
        return self.run(bundle, coverage_threshold=coverage_threshold)

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


class CoverageAggregator:
    """Pool case outcomes across multiple bundle runs without averaging rates."""

    def __init__(self) -> None:
        self._reports: list[RunReport] = []

    def add(self, report: RunReport) -> None:
        self._reports.append(report)

    @property
    def report(self) -> RunReport:
        return RunReport(tuple(result for report in self._reports for result in report.results))

    def enforce(self, threshold: float) -> RunReport:
        report = self.report
        _enforce_threshold(report, threshold)
        return report


def match_case(case: Case, actual: Any) -> tuple[bool, str | None]:
    for predicate in case.predicates:
        if evaluate_predicate(predicate.expression, actual):
            return True, f"predicate:{predicate.id}"
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


def _enforce_threshold(report: RunReport, threshold: float) -> None:
    if not 0 <= threshold <= 100:
        raise ValueError("coverage threshold must be between 0 and 100")
    if report.coverage < threshold:
        raise CoverageThresholdError(report, threshold)
