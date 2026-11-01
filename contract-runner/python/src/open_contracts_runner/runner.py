"""Case execution and predicate-first matching."""

from __future__ import annotations

from concurrent.futures import ThreadPoolExecutor
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
            sequence_passed = True
            if case.steps:
                observed, sequence_passed = self._run_sequence(target, case)
            else:
                observed = self.executors.get(target.kind).execute(target, case)
            actual = normalize_json_value(observed, "actual")
            passed, matched_by = match_case(case, actual)
            passed = passed and sequence_passed
            return CaseResult(target.id, case.id, actual, passed, matched_by if passed else None)
        except Exception as exc:
            return CaseResult(
                target.id,
                case.id,
                error=f"{type(exc).__name__}: {exc}",
            )

    def _run_sequence(self, target: Target, case: Case) -> tuple[dict[str, Any], bool]:
        results: dict[str, Any] = {}
        captures: dict[str, Any] = {}
        checks: list[dict[str, Any]] = []
        executor = self.executors.get(target.kind)
        _validate_sequence(case.steps)
        step_indices = {
            step["id"]: index
            for index, step in enumerate(
                step for item in case.steps for step in item.get("parallel", [item])
            )
        }

        def execute(step: dict[str, Any]) -> tuple[str, Any, bool, dict[str, Any]]:
            step_id = step["id"]
            step_input = _resolve_references(step["input"], case.input, captures)
            observed = normalize_json_value(
                executor.execute(target, Case(
                    f"{case.id}/{step_id}",
                    step_input,
                    context={
                        "case_id": case.id,
                        "step_id": step_id,
                        "step_index": step_indices[step_id],
                        "captures": dict(captures),
                    },
                )),
                f"step {step_id} actual",
            )
            step_case = Case(
                step_id,
                step_input,
                step.get("expected"),
                step.get("predicate"),
            )
            passed, matched_by = (
                match_case(step_case, observed)
                if "expected" in step or "predicate" in step
                else (True, None)
            )
            step_captures = {
                name: _value_at_pointer(observed, pointer)
                for name, pointer in step.get("capture", {}).items()
            }
            if any(value is _MISSING for value in step_captures.values()):
                missing = next(name for name, value in step_captures.items() if value is _MISSING)
                raise ValueError(f"capture {missing} was not found")
            check = {"id": step_id, "passed": passed, "matched_by": matched_by}
            return step_id, observed, passed, {"values": step_captures, "check": check}

        for item in case.steps:
            group = item.get("parallel")
            group_steps = group if group is not None else [item]
            if group is None:
                completed = [execute(item)]
            else:
                with ThreadPoolExecutor(max_workers=len(group_steps)) as pool:
                    completed = list(pool.map(execute, group_steps))
            capture_names = [name for _, _, _, extra in completed for name in extra["values"]]
            if len(set(capture_names)) != len(capture_names) or any(name in captures for name in capture_names):
                raise ValueError("duplicate sequence capture")
            for step_id, result, _passed, extra in completed:
                results[step_id] = result
                captures.update(extra["values"])
                checks.append(extra["check"])
        return {"input": case.input, "steps": results, "captures": captures, "checks": checks}, all(
            check["passed"] for check in checks
        )


_MISSING = object()


def _validate_sequence(steps: tuple[dict[str, Any], ...]) -> None:
    ids: set[str] = set()
    captures: set[str] = set()
    for item in steps:
        for step in item.get("parallel", [item]):
            step_id = step["id"]
            if step_id in ids:
                raise ValueError(f"duplicate sequence step id: {step_id}")
            ids.add(step_id)
            for name in step.get("capture", {}):
                if name in captures:
                    raise ValueError(f"duplicate sequence capture: {name}")
                captures.add(name)


def _value_at_pointer(value: Any, pointer: str) -> Any:
    if not isinstance(pointer, str) or (pointer and not pointer.startswith("/")):
        raise ValueError("capture path must be a JSON Pointer")
    current = value
    if pointer == "":
        return current
    for part in pointer[1:].split("/"):
        key = part.replace("~1", "/").replace("~0", "~")
        if isinstance(current, dict) and key in current:
            current = current[key]
        elif isinstance(current, list) and key.isdigit() and str(int(key)) == key and int(key) < len(current):
            current = current[int(key)]
        else:
            return _MISSING
    return current


def _resolve_references(value: Any, input_value: Any, captures: dict[str, Any]) -> Any:
    if isinstance(value, list):
        return [_resolve_references(item, input_value, captures) for item in value]
    if isinstance(value, dict):
        if set(value) == {"$capture"}:
            name = value["$capture"]
            if name not in captures:
                raise ValueError(f"unknown sequence capture: {name}")
            return captures[name]
        if set(value) == {"$input"}:
            result = _value_at_pointer(input_value, value["$input"])
            if result is _MISSING:
                raise ValueError("input reference was not found")
            return result
        return {key: _resolve_references(item, input_value, captures) for key, item in value.items()}
    return value


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
