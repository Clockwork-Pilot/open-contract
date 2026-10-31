from __future__ import annotations

import json
from http.server import BaseHTTPRequestHandler, HTTPServer
import threading
from pathlib import Path
import sys
from types import MappingProxyType
from typing import Any

import pytest
import jsonschema
from referencing import Registry, Resource

from open_contracts_runner import (
    Case,
    ContractRunner,
    ExecutorRegistry,
    HttpExecutor,
    Target,
    load_bundle,
)


class EchoExecutor:
    def __init__(self) -> None:
        self.calls: list[tuple[str, Any]] = []

    def execute(self, target: Target, case: Case) -> Any:
        self.calls.append((target.id, case.input))
        return case.input


def registry_for(executor: Any, kind: str = "echo") -> ExecutorRegistry:
    registry = ExecutorRegistry()
    registry.register(kind, executor)
    return registry


def test_reads_json_and_yaml_files(tmp_path: Path) -> None:
    raw = {"version": 1, "kind": "echo", "cases": [
        {"id": "one", "input": {"value": 1}, "expected": {"value": 1}}
    ]}
    json_path = tmp_path / "bundle.json"
    yaml_path = tmp_path / "bundle.yaml"
    json_path.write_text(json.dumps(raw), encoding="utf-8")
    yaml_path.write_text("version: 1\nkind: echo\ncases:\n  - id: one\n    input: {value: 1}\n    expected: {value: 1}\n", encoding="utf-8")

    assert load_bundle(json_path) == load_bundle(yaml_path)


def test_yaml_reader_rejects_values_outside_json_data_model(tmp_path: Path) -> None:
    yaml_path = tmp_path / "not-json.yaml"
    yaml_path.write_text(
        "version: 1\nkind: echo\ncases:\n  - id: date\n    input: null\n    expected: 2026-10-31\n",
        encoding="utf-8",
    )
    with pytest.raises(ValueError, match="non-JSON value date"):
        load_bundle(yaml_path)


def test_custom_reader_object_and_function_return_normalized_bundle() -> None:
    bundle = {"version": 1, "kind": "echo", "cases": [
        {"id": "one", "input": 1, "expected": 1}
    ]}

    class Reader:
        def read(self, source: str) -> Any:
            assert source == "native-config"
            return bundle

    assert load_bundle("native-config", Reader()).targets[0].cases[0].id == "one"
    assert load_bundle("native-config", lambda _: bundle).targets[0].kind == "echo"


def test_grouped_targets_execute_every_case_and_report_per_target() -> None:
    executor = EchoExecutor()
    runner = ContractRunner(executors=registry_for(executor))
    report = runner.run({"version": 1, "targets": [
        {"id": "first", "kind": "echo", "cases": [
            {"id": "a", "input": {"n": 1}, "expected": {"n": 1}},
            {"id": "b", "input": {"n": 2}, "expected": {"n": 2}},
        ]},
        {"id": "second", "kind": "echo", "cases": [
            {"id": "c", "input": {"n": 3}, "expected": {"n": 3}},
        ]},
    ]})

    assert report.total == report.passed == 3
    assert set(report.by_target) == {"first", "second"}
    assert [case.case_id for case in report.by_target["first"]] == ["a", "b"]
    assert len(executor.calls) == 3
    assert report.to_dict()["targets"]["second"]["cases"][0]["actual"] == {"n": 3}


def test_single_predicate_supports_logical_operators() -> None:
    runner = ContractRunner(executors=registry_for(EchoExecutor()))
    report = runner.run({"version": 1, "kind": "echo", "cases": [{
        "id": "predicate-match",
        "input": {"status": "ready", "count": 3},
        "predicate": "NOT (.count < 3) AND (.status == \"ready\" OR .status == \"warm\")",
    }]})

    assert report.results[0].passed
    assert report.results[0].matched_by == "predicate"


def test_exact_match_fallback_and_json_type_sensitivity() -> None:
    runner = ContractRunner(executors=registry_for(EchoExecutor()))
    report = runner.run({"version": 1, "kind": "echo", "cases": [
        {"id": "exact", "input": {"ok": True}, "expected": {"ok": True},
        },
        {"id": "bool-is-not-number", "input": True, "expected": 1},
    ]})

    assert report.results[0].passed and report.results[0].matched_by == "exact"
    assert not report.results[1].passed


def test_command_executor_runs_a_process_per_case() -> None:
    runner = ContractRunner()
    command = [sys.executable, "-c", "import sys; print(sys.stdin.read())"]
    report = runner.run({"version": 1, "targets": [{
        "id": "echo-process", "kind": "command", "config": {"command": command},
        "cases": [
            {"id": "one", "input": {"n": 1}, "expected": {"n": 1}},
            {"id": "two", "input": {"n": 2}, "expected": {"n": 2}},
        ],
    }]})

    assert report.passed == 2
    assert [result.actual for result in report.results] == [{"n": 1}, {"n": 2}]


def test_http_executor_normalizes_status_and_json_response() -> None:
    received: list[dict[str, Any]] = []

    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            size = int(self.headers["Content-Length"])
            received.append(json.loads(self.rfile.read(size)))
            body = b'{"accepted":true}'
            self.send_response(201)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_: Any) -> None:
            pass

    server = HTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        result = HttpExecutor().execute(
            Target("http", "http", (), {"url": f"http://127.0.0.1:{server.server_port}/"}),
            Case("one", {"hello": "world"}, {"status": 201, "body": {"accepted": True}}),
        )
    finally:
        server.shutdown()
        thread.join(timeout=2)
        server.server_close()

    assert received == [{"hello": "world"}]
    assert result == {"status": 201, "body": {"accepted": True}}


def test_executor_errors_fail_even_when_expected_is_null() -> None:
    class Broken:
        def execute(self, target: Target, case: Case) -> Any:
            raise RuntimeError("failed to execute")

    report = ContractRunner(executors=registry_for(Broken())).run({
        "version": 1,
        "kind": "echo",
        "cases": [{"id": "broken", "input": None, "expected": None}],
    })
    assert not report.results[0].passed
    assert report.results[0].error == "RuntimeError: failed to execute"


def test_custom_executor_observation_is_normalized_to_json_data() -> None:
    class MappingExecutor:
        def execute(self, target: Target, case: Case) -> Any:
            return MappingProxyType({"result": MappingProxyType({"ok": True})})

    report = ContractRunner(executors=registry_for(MappingExecutor())).run({
        "version": 1,
        "kind": "echo",
        "cases": [{
            "id": "normalized-observation",
            "input": {},
            "expected": {},
            "predicate": ".result.ok == true",
        }],
    })

    assert report.results[0].actual == {"result": {"ok": True}}
    assert report.results[0].passed


def test_non_json_custom_observation_becomes_case_error() -> None:
    from datetime import date

    class DateExecutor:
        def execute(self, target: Target, case: Case) -> Any:
            return date(2026, 10, 31)

    report = ContractRunner(executors=registry_for(DateExecutor())).run({
        "version": 1,
        "kind": "echo",
        "cases": [{"id": "date-result", "input": None, "expected": None}],
    })

    assert not report.results[0].passed
    assert report.results[0].error == "ValueError: actual contains non-JSON value date"


def test_report_counts_case_outcomes_without_claiming_code_coverage() -> None:
    runner = ContractRunner(executors=registry_for(EchoExecutor()))
    report = runner.run({"version": 1, "kind": "echo", "cases": [
        {"id": "pass", "input": 1, "expected": 1},
        {"id": "fail", "input": 1, "expected": 2},
    ]})

    assert report.passed == 1
    assert report.total == 2
    assert "coverage" not in report.to_dict()


@pytest.mark.parametrize("bad", [
    {},
    {"version": 2, "cases": []},
    {"version": 1, "cases": [], "targets": []},
    {"version": 1, "cases": [{"id": "missing-input", "expected": None}]},
    {"version": 1, "cases": [{"id": "missing-matcher", "input": None}]},
    {"version": 1, "cases": [{"id": "both-matchers", "input": None, "expected": None, "predicate": "true"}]},
])
def test_rejects_invalid_bundle_shapes(bad: dict[str, Any]) -> None:
    with pytest.raises(ValueError):
        load_bundle(bad)


def test_v1_schema_accepts_grouped_bundle_and_string_predicates() -> None:
    root = Path(__file__).resolve().parents[1]
    bundle_schema = json.loads((root / "schemas/bundle.v1.schema.json").read_text(encoding="utf-8"))
    predicate_schema_path = root.parents[1] / "bootstrap-contracts/predicates/predicate-language.schema.json"
    predicate_schema = json.loads(predicate_schema_path.read_text(encoding="utf-8"))
    registry = Registry().with_resources((
        (schema["$id"], Resource.from_contents(schema))
        for schema in (bundle_schema, predicate_schema)
    ))
    instance = {
        "version": 1,
        "targets": [{
            "id": "hook-a",
            "kind": "hooks",
            "cases": [{
                "id": "event",
                "input": {"event": {"name": "start"}},
                "predicate": ".event.name == \"start\"",
            }],
        }],
    }
    jsonschema.Draft202012Validator(bundle_schema, registry=registry).validate(instance)
    instance["targets"][0]["cases"][0]["predicate"] = {"event": "start"}
    with pytest.raises(jsonschema.ValidationError):
        jsonschema.Draft202012Validator(bundle_schema, registry=registry).validate(instance)
