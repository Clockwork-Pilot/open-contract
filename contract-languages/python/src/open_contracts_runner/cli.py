"""Command line interface for running normalized contract datasets."""

from __future__ import annotations

import argparse
import importlib
import importlib.util
import threading
import json
import os
import shutil
import secrets
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any, Sequence

import yaml

from .executors import ExecutorRegistry, FunctionExecutor
from .manifest import read_manifest, stage_contract_tree
from .model import Bundle, RunReport, Target
from .reader import JsonYamlReader, load_bundle
from .runner import ContractRunner


_MANIFEST_COVERAGE_PATHS: dict[str, str] = {}


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="open-contracts")
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="run a JSON or YAML dataset")
    run.add_argument("dataset", type=Path, help="path to a normalized dataset")
    run.add_argument("--reader", metavar="MODULE:CALLABLE", help="pure adapter accepting parsed JSON/YAML data and returning a normalized dataset")
    run.add_argument("--executor", metavar="MODULE:CALLABLE", help="evaluator called once per case with its normalized dataset entry")
    run.add_argument(
        "--quiet",
        action="store_true",
        help="print a concise summary and expected/actual values for failed cases",
    )
    run.add_argument("--coverage", action="store_true", help="collect Python coverage while running this dataset")
    run.add_argument("--coverage-source", action="append", default=[], metavar="MODULE_OR_PATH", help="source module or path to measure; may be repeated")
    run.add_argument("--coverage-file", type=Path, default=Path(".coverage"), help="coverage data file (default: .coverage)")
    run.add_argument("--coverage-append", action="store_true", help="include coverage data already in --coverage-file")
    run.add_argument("--coverage-threshold", type=_percentage, help="fail when collected coverage is below this percentage")
    manifest_run = commands.add_parser("run-manifest", help="run a contract package manifest")
    manifest_run.add_argument("manifest", type=Path)
    manifest_run.add_argument("--isolation", choices=("none", "bwrap", "landlock"), required=True)
    manifest_run.add_argument("--quiet", action="store_true")
    manifest_run.add_argument("--coverage", action="store_true")
    manifest_run.add_argument("--coverage-source", action="append", default=[])
    manifest_run.add_argument("--coverage-file", type=Path, default=Path(".coverage"))
    manifest_run.add_argument("--coverage-append", action="store_true")
    manifest_run.add_argument("--coverage-threshold", type=_percentage)
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
    return _run_manifest(args) if args.command == "run-manifest" else _run_dataset(args)


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
            bundle_source = reader(source_data)
        else:
            bundle_source = args.dataset
        bundle = load_bundle(bundle_source)
        report = runner.run(bundle_source)
    except (OSError, ValueError, ImportError, AttributeError, yaml.YAMLError) as exc:
        print(f"open-contracts: {exc}", file=sys.stderr)
        return 2

    if args.quiet:
        print(f"Cases passed: {report.passed}/{report.total}")
        cases = {
            (target.id, case.id): case
            for target in bundle.targets
            for case in target.cases
        }
        for result in report.results:
            if not result.passed:
                print(f"FAIL {result.target_id}/{result.case_id}", file=sys.stderr)
                if result.error:
                    print(f"  error: {result.error}", file=sys.stderr)
                    continue
                case = cases[(result.target_id, result.case_id)]
                if case.predicate is not None:
                    print(f"  predicate: {case.predicate}", file=sys.stderr)
                else:
                    expected = json.dumps(case.expected, ensure_ascii=False, sort_keys=True)
                    print(f"  expected: {expected}", file=sys.stderr)
                actual = json.dumps(result.actual, ensure_ascii=False, sort_keys=True)
                print(f"  actual:   {actual}", file=sys.stderr)
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
    if not sources:
        for plugin_spec in (getattr(args, "reader", None), getattr(args, "executor", None)):
            if plugin_spec:
                sources.append(plugin_spec.partition(":")[0])
        if args.command == "run-manifest":
            sources.extend(_collect_manifest_sources(args.manifest))
    if not sources:
        print("open-contracts: coverage requires --coverage-source or a custom reader/executor", file=sys.stderr)
        return 2
    try:
        args.coverage_file.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(
            mode="w", encoding="utf-8", suffix=".ini", prefix="open-contract-coverage-",
        ) as config_file:
            config_file.write("[run]\n")
            config_file.flush()
            coverage = Coverage(
                data_file=str(args.coverage_file), source=None if args.command == "run-manifest" else sources, branch=True,
                config_file=config_file.name,
            )
            coverage.set_option("run:patch", ["subprocess"])
            coverage.set_option("run:parallel", True)
            if args.coverage_append:
                # Fold subprocess data from earlier dataset runs into the base
                # file before this run starts collecting with parallel data files.
                coverage.load()
                coverage.combine()
            else:
                coverage.erase()
            coverage.start()
            try:
                result = _run_manifest(args) if args.command == "run-manifest" else _run_dataset(args)
            finally:
                coverage.stop()
                coverage.save()
            # Native subprocess collection writes parallel files. Consolidate them
            # so --coverage-append and coverage-report see the full dataset run.
            coverage.combine()
            if args.command == "run-manifest":
                _remap_manifest_coverage(coverage, _MANIFEST_COVERAGE_PATHS)
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

        coverage = Coverage(
            data_file=str(data_file), source=sources or None, branch=True,
            config_file=False,
        )
        coverage.set_option("run:parallel", True)
        coverage.load()
        coverage.combine()
        percentage = coverage.report(
            include=_coverage_include_patterns(sources) or None,
            file=sys.stderr,
        )
    except (OSError, CoverageException) as exc:
        print(f"open-contracts: coverage error: {exc}", file=sys.stderr)
        return 2
    if threshold is not None and percentage + 1e-9 < threshold:
        print(f"Coverage {percentage:.2f}% is below required {threshold:.2f}%", file=sys.stderr)
        return 1
    if threshold is not None:
        print(f"Coverage threshold passed: {percentage:.2f}% >= {threshold:.2f}%", file=sys.stderr)
    return 0


def _remap_manifest_coverage(coverage: Any, paths: dict[str, str]) -> None:
    """Rewrite staged validator filenames to their package source paths."""
    data = coverage.get_data()
    has_arcs = data.has_arcs()
    remapped_arcs: dict[str, set[tuple[int, int]]] = {}
    remapped_lines: dict[str, set[int]] = {}
    tracers: dict[str, str] = {}
    for staged, original in paths.items():
        if staged not in data.measured_files():
            continue
        target = original
        if has_arcs:
            remapped_arcs.setdefault(target, set()).update(data.arcs(staged) or [])
        else:
            remapped_lines.setdefault(target, set()).update(data.lines(staged) or [])
        tracer = data.file_tracer(staged)
        if tracer:
            tracers[target] = tracer
    for filename in data.measured_files():
        if filename in paths:
            continue
        if has_arcs:
            remapped_arcs.setdefault(filename, set()).update(data.arcs(filename) or [])
        else:
            remapped_lines.setdefault(filename, set()).update(data.lines(filename) or [])
        tracer = data.file_tracer(filename)
        if tracer:
            tracers[filename] = tracer
    data.erase()
    if has_arcs:
        data.add_arcs({name: sorted(arcs) for name, arcs in remapped_arcs.items()})
    else:
        data.add_lines({name: sorted(lines) for name, lines in remapped_lines.items()})
    if tracers:
        data.add_file_tracers(tracers)
    data.write()


def _run_manifest(args: argparse.Namespace) -> int:
    try:
        coverage_file = args.coverage_file if args.coverage or args.coverage_threshold is not None else None
        report = _execute_manifest(args.manifest, args.isolation, set(), coverage_file)
    except (OSError, ValueError, ImportError, AttributeError, yaml.YAMLError, RuntimeError, subprocess.SubprocessError) as exc:
        print(f"open-contracts: {exc}", file=sys.stderr)
        return 2
    if args.quiet:
        print(f"Cases passed: {report.passed}/{report.total}")
        for result in report.results:
            if not result.passed:
                print(f"FAIL {result.target_id}/{result.case_id}: {result.error or result.actual}", file=sys.stderr)
    else:
        print(json.dumps(report.to_dict(), indent=2, ensure_ascii=False))
    return 0 if report.passed == report.total else 1


def _execute_manifest(path: Path, backend: str, stack: set[Path], coverage_file: Path | None = None) -> RunReport:
    """Verify contract outputs outside one isolation boundary."""
    manifest_path = path.resolve(strict=True)
    if manifest_path in stack:
        raise ValueError(f"nested contract cycle detected at {manifest_path}")
    manifest, source_root = read_manifest(manifest_path)
    with stage_contract_tree(manifest, source_root, manifest_path) as (staged_root, read_paths, write_paths, source_map):
        shutil.copyfile(manifest_path, staged_root / manifest_path.name)
        read_paths.append(manifest_path.name)
        coverage_config = None
        if coverage_file is not None:
            coverage_config = staged_root / ".open-contract-coverage.ini"
            coverage_config.write_text("[run]\nbranch = true\nparallel = true\npatch = subprocess\n", encoding="utf-8")
            read_paths.append(coverage_config.name)
        internal = [sys.executable, "-c", _executor_entrypoint(), str(staged_root)]
        if coverage_file is not None:
            internal.append(str(coverage_config))
        launcher = Path(__file__).resolve().parents[4] / "bin" / "contract-isolate"
        argv = [str(launcher), "--backend", backend, "--package-root", str(staged_root)]
        guest_root = f"/contract/{secrets.token_hex(12)}"
        if backend == "bwrap":
            argv.extend(("--guest-root", guest_root))
        _MANIFEST_COVERAGE_PATHS.clear()
        for staged_path, original_path in source_map.items():
            actual_path = staged_root / Path(staged_path).relative_to(staged_root)
            if backend == "bwrap":
                actual_path = Path(guest_root) / actual_path.relative_to(staged_root)
            _MANIFEST_COVERAGE_PATHS[str(actual_path)] = original_path
        for name in read_paths:
            argv.extend(("--read", name))
        for name in write_paths:
            argv.extend(("--write", name))
        env = dict(os.environ)
        if coverage_file is not None:
            env["COVERAGE_FILE"] = str(coverage_file.resolve())
        argv.extend(("--runtime-profile", "python"))
        process = subprocess.Popen([*argv, "--", *internal], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                   stderr=subprocess.PIPE, text=True, env=env, bufsize=1)
        assert process.stdin and process.stdout and process.stderr
        lock = threading.Lock()

        class IsolatedExecutor:
            def execute(self, validator: str, value: Any) -> Any:
                request = json.dumps({"validator": validator, "input": value}, ensure_ascii=False)
                with lock:
                    process.stdin.write(request + "\n")
                    process.stdin.flush()
                    line = process.stdout.readline()
                if not line:
                    raise RuntimeError("isolated executor stopped without returning output")
                response = json.loads(line)
                if "error" in response:
                    raise RuntimeError(response["error"])
                return response.get("output")

        contracts: list[tuple[dict[str, Any], Path, str]] = []
        def collect(current: Path, prefix: str, parents: set[Path]) -> None:
            real = current.resolve(strict=True)
            if real in parents:
                raise ValueError(f"nested contract cycle detected at {real}")
            child_manifest, child_root = read_manifest(real)
            contracts.append((child_manifest, child_root, prefix))
            for child in child_manifest["files"].get("nested_contracts", []):
                child_prefix = f"{child_manifest['name']}/{Path(child).parent.name or Path(child).stem}"
                collect(child_root / child, f"{prefix}/{child_prefix}".strip("/"), {*parents, real})

        collect(manifest_path, "", set())
        executor = IsolatedExecutor()
        registry = ExecutorRegistry.with_builtins()
        targets: list[Target] = []
        for index, (contract, source_root, prefix) in enumerate(contracts):
            kind = f"contract_{index}"
            validator_source = (source_root / contract["files"]["validator"]).resolve(strict=True)
            staged_validator = next((Path(name) for name, original in source_map.items() if Path(original) == validator_source), None)
            if staged_validator is None:
                raise ValueError(f"validator is not staged: {validator_source}")
            validator_relative = staged_validator.relative_to(staged_root).as_posix()
            registry.register(kind, FunctionExecutor(lambda value, _context=None, validator=validator_relative: executor.execute(validator, value)))
            bundle = load_bundle(source_root / contract["files"]["dataset"])
            for target in bundle.targets:
                target_id = f"{prefix}/{target.id}" if prefix else target.id
                targets.append(Target(target_id, kind, target.cases, target.config))
        try:
            report = ContractRunner(executors=registry).run(Bundle(1, tuple(targets)))
        finally:
            process.stdin.close()
        stderr = process.stderr.read()
        if process.wait(timeout=600):
            raise RuntimeError(f"isolated contract executor exited {process.returncode}: {stderr.strip()}")
        return report


def _executor_entrypoint() -> str:
    return """import json, os, sys
cov = None
if os.environ.get('COVERAGE_FILE') and len(sys.argv) > 2:
    from coverage import Coverage
    cov = Coverage(data_file=os.environ['COVERAGE_FILE'], config_file=sys.argv[2], branch=True)
    cov.set_option('run:parallel', True)
    cov.set_option('run:patch', ['subprocess'])
    cov.start()
try:
    from open_contracts_runner.execution import serve
    raise SystemExit(serve(sys.argv[1]))
finally:
    if cov:
        cov.stop()
        cov.save()
"""


def _collect_manifest_sources(path: Path, visited: set[Path] | None = None) -> list[str]:
    visited = visited or set()
    resolved = path.resolve(strict=True)
    if resolved in visited:
        return []
    visited.add(resolved)
    manifest, root = read_manifest(resolved)
    sources = [str(root / manifest["files"]["validator"])]
    sources.extend(str(root / name) for name in manifest["files"].get("support", []))
    for child in manifest["files"].get("nested_contracts", []):
        sources.extend(_collect_manifest_sources(root / child, visited))
    return list(dict.fromkeys(sources))


def _coverage_include_patterns(sources: list[str]) -> list[str]:
    """Translate report source paths or module names into file include patterns."""
    patterns: list[str] = []
    for source in sources:
        path = Path(source).expanduser()
        if path.exists():
            path = path.resolve()
            patterns.append(str(path / "*") if path.is_dir() else str(path))
            continue
        try:
            module = importlib.util.find_spec(source)
        except (ImportError, ValueError):
            module = None
        if module is not None and module.submodule_search_locations:
            patterns.extend(
                str(Path(location).resolve() / "*")
                for location in module.submodule_search_locations
            )
        elif module is not None and module.origin:
            patterns.append(str(Path(module.origin).resolve()))
        else:
            patterns.append(source)
    return patterns


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
