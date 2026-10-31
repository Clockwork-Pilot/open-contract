"""Command line interface for running normalized contract datasets."""

from __future__ import annotations

import argparse
import importlib
import json
import sys
from pathlib import Path
from typing import Sequence

import yaml

from .executors import ExecutorRegistry, FunctionExecutor
from .reader import JsonYamlReader
from .runner import ContractRunner


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="open-contracts")
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="run a JSON or YAML dataset")
    run.add_argument("dataset", type=Path, help="path to a normalized dataset")
    run.add_argument("--reader", metavar="MODULE:CALLABLE", help="pure adapter accepting parsed JSON/YAML data and returning a normalized dataset")
    run.add_argument("--executor", metavar="MODULE:CALLABLE", help="evaluator called once per case with its normalized dataset entry")
    run.add_argument("--quiet", action="store_true", help="print a concise result summary instead of the JSON report")
    run.add_argument("--coverage", action="store_true", help="collect Python coverage while running this dataset")
    run.add_argument("--coverage-source", action="append", default=[], metavar="MODULE_OR_PATH", help="source module or path to measure; may be repeated")
    run.add_argument("--coverage-file", type=Path, default=Path(".coverage"), help="coverage data file (default: .coverage)")
    run.add_argument("--coverage-append", action="store_true", help="include coverage data already in --coverage-file")
    run.add_argument("--coverage-threshold", type=_percentage, help="fail when collected coverage is below this percentage")

    coverage_report = commands.add_parser("coverage-report", help="report or enforce a saved coverage data file")
    coverage_report.add_argument("--coverage-file", type=Path, default=Path(".coverage"), help="coverage data file (default: .coverage)")
    coverage_report.add_argument("--coverage-source", action="append", default=[], metavar="MODULE_OR_PATH", help="limit report to these sources; may be repeated")
    coverage_report.add_argument("--coverage-threshold", type=_percentage, help="fail when coverage is below this percentage")
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.command == "coverage-report":
        return _coverage_report(args.coverage_file, args.coverage_source, args.coverage_threshold)
    if args.coverage or args.coverage_threshold is not None:
        return _run_with_coverage(args)
    return _run_dataset(args)


def _run_dataset(args: argparse.Namespace) -> int:
    try:
        sys.path.insert(0, str(args.dataset.resolve().parent))
        executors = ExecutorRegistry.with_builtins()
        if args.executor:
            executors.register("custom", FunctionExecutor(_load_callable(args.executor)))
        reader = _load_callable(args.reader) if args.reader else None
        runner = ContractRunner(executors=executors)
        if reader:
            source_data = JsonYamlReader().read(args.dataset)
            report = runner.run(reader(source_data))
        else:
            report = runner.run(args.dataset)
    except (OSError, ValueError, ImportError, AttributeError, yaml.YAMLError) as exc:
        print(f"open-contracts: {exc}", file=sys.stderr)
        return 2

    if args.quiet:
        print(f"Cases passed: {report.passed}/{report.total}")
        for result in report.results:
            if not result.passed:
                detail = result.error or "case did not match expected or predicate"
                print(f"FAIL {result.target_id}/{result.case_id}: {detail}", file=sys.stderr)
    else:
        print(json.dumps(report.to_dict(), indent=2, ensure_ascii=False))
    return 0 if report.passed == report.total else 1


def _run_with_coverage(args: argparse.Namespace) -> int:
    try:
        from coverage import Coverage
        from coverage.exceptions import CoverageException
    except ImportError:
        print("open-contracts: coverage collection requires the 'coverage' dependency", file=sys.stderr)
        return 2

    sources = list(args.coverage_source)
    for plugin_spec in (args.reader, args.executor):
        if plugin_spec:
            module_name = plugin_spec.partition(":")[0]
            if module_name not in sources:
                sources.append(module_name)
    if not sources:
        print("open-contracts: coverage requires --coverage-source or a custom reader/executor", file=sys.stderr)
        return 2
    try:
        coverage = Coverage(data_file=str(args.coverage_file), source=sources)
        if args.coverage_append and args.coverage_file.exists():
            coverage.load()
        coverage.start()
        try:
            result = _run_dataset(args)
        finally:
            coverage.stop()
            coverage.save()
        percentage = coverage.report(file=sys.stderr)
    except (OSError, CoverageException) as exc:
        print(f"open-contracts: coverage error: {exc}", file=sys.stderr)
        return 2

    if args.coverage_threshold is not None and percentage + 1e-9 < args.coverage_threshold:
        print(f"Coverage {percentage:.2f}% is below required {args.coverage_threshold:.2f}%", file=sys.stderr)
        return 1
    if args.coverage_threshold is not None:
        print(f"Coverage threshold passed: {percentage:.2f}% >= {args.coverage_threshold:.2f}%", file=sys.stderr)
    return result


def _coverage_report(data_file: Path, sources: list[str], threshold: float | None) -> int:
    try:
        from coverage import Coverage
        from coverage.exceptions import CoverageException

        coverage = Coverage(data_file=str(data_file), source=sources or None)
        coverage.load()
        percentage = coverage.report(file=sys.stderr)
    except (OSError, CoverageException) as exc:
        print(f"open-contracts: coverage error: {exc}", file=sys.stderr)
        return 2
    if threshold is not None and percentage + 1e-9 < threshold:
        print(f"Coverage {percentage:.2f}% is below required {threshold:.2f}%", file=sys.stderr)
        return 1
    if threshold is not None:
        print(f"Coverage threshold passed: {percentage:.2f}% >= {threshold:.2f}%", file=sys.stderr)
    return 0


def _percentage(value: str) -> float:
    try:
        percentage = float(value)
    except ValueError as exc:
        raise argparse.ArgumentTypeError("coverage percentage must be a number") from exc
    if not 0 <= percentage <= 100:
        raise argparse.ArgumentTypeError("coverage percentage must be between 0 and 100")
    return percentage


def _load_callable(spec: str):
    module_name, separator, attribute = spec.partition(":")
    if not separator or not module_name or not attribute:
        raise ValueError(f"plugin must use MODULE:CALLABLE syntax: {spec}")
    value = getattr(importlib.import_module(module_name), attribute)
    if not callable(value):
        raise ValueError(f"plugin is not callable: {spec}")
    return value
