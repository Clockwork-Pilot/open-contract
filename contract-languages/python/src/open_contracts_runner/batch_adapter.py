"""Execute one whole validator batch and return outputs plus native coverage."""

from __future__ import annotations

import asyncio
import contextlib
import importlib.util
import inspect
import json
import sys
from pathlib import Path


def main() -> int:
    try:
        request = json.load(sys.stdin)
        validator = Path(request["validator"]).resolve(strict=True)
        root = Path.cwd().resolve(strict=True)
        if not validator.is_relative_to(root):
            raise ValueError("validator path escaped staged package root")
        package_root = (root / request.get("package", "")).resolve(strict=True)
        if not package_root.is_relative_to(root):
            raise ValueError("contract package path escaped staged root")
    except Exception as exc:
        print(json.dumps({"results": [], "coverage": None, "error": f"{type(exc).__name__}: {exc}"}))
        return 2

    try:
        from coverage import Coverage
    except ImportError as exc:
        print(json.dumps({"results": [], "coverage": None, "error": f"coverage unavailable: {exc}"}))
        return 2

    coverage = Coverage(data_file=None, branch=True, config_file=False)
    results = []
    keys_by_context = {}
    setup_context = "open-contract:setup"
    coverage.start()
    try:
        with contextlib.redirect_stdout(sys.stderr):
            coverage.switch_context(setup_context)
            try:
                executor = _load_validator(validator, root)
            except Exception as exc:
                for item in request.get("requests", []):
                    results.append({"key": item["key"], "error": f"{type(exc).__name__}: {exc}"})
            else:
                for item in request.get("requests", []):
                    context = _coverage_context(item["key"])
                    keys_by_context[context] = item["key"]
                    coverage.switch_context(context)
                    try:
                        output = executor(item["input"])
                        if inspect.isawaitable(output):
                            output = asyncio.run(output)
                        output = json.loads(json.dumps(output, ensure_ascii=False, allow_nan=False))
                        results.append({"key": item["key"], "output": output})
                    except Exception as exc:
                        results.append({"key": item["key"], "error": f"{type(exc).__name__}: {exc}"})
    finally:
        coverage.stop()

    data = coverage.get_data()
    files = {}
    coverage_sources = request.get("coverage_sources", [])
    contexts_by_file = {}
    for name in sorted(data.measured_files()):
        source = Path(name).resolve()
        if not source.is_relative_to(root):
            continue
        relative = source.relative_to(root).as_posix()
        files[relative] = {"lines": data.lines(name) or [], "arcs": data.arcs(name) or []}
        contexts_by_file[name] = data.contexts_by_lineno(name)
    coverage_lines = _normalized_coverage_lines(
        coverage, package_root, coverage_sources
    )
    native = {"format": "coverage.py", "sources": sorted(files), "files": files}
    scopes = []
    for context, key in keys_by_context.items():
        scoped_files = []
        for name, lines_by_context in contexts_by_file.items():
            source = Path(name).resolve()
            if not source.is_relative_to(package_root):
                continue
            relative = source.relative_to(package_root).as_posix()
            if not _is_declared_source(relative, coverage_sources):
                continue
            lines = sorted(
                line for line, contexts in lines_by_context.items()
                if context in contexts or setup_context in contexts
            )
            if lines:
                scoped_files.append({"path": relative, "lines": lines})
        scopes.append({"key": key, "files": scoped_files})
    additional_report = None
    additional_coverage_lines = []
    supplemental_tests = request.get("supplemental_tests", [])
    if supplemental_tests:
        dataset_percent = _coverage_percent(coverage_lines)
        if dataset_percent >= 90.0:
            additional_report, additional_coverage_lines = _run_supplemental_tests(
                supplemental_tests, root, package_root, coverage_sources
            )
        else:
            additional_report = {
                "status": "skipped",
                "executed": False,
                "passed": True,
                "reason": f"dataset-only line coverage {dataset_percent:.1f}% is below the 90% threshold",
                "test_files": supplemental_tests,
            }
    print(json.dumps({
        "results": results,
        "coverage": native,
        "coverage_scopes": scopes,
        "coverage_lines": sorted(coverage_lines, key=lambda item: item["path"]),
        "additional_test_coverage": additional_report,
        "additional_coverage_lines": additional_coverage_lines,
    }, ensure_ascii=False))
    return 0


def _coverage_context(key):
    return "open-contract:" + json.dumps(key, sort_keys=True, separators=(",", ":"))


def _is_declared_source(path, declared):
    return any(path == entry or path.startswith(entry.rstrip("/") + "/") for entry in declared)


def _declared_source_files(package_root: Path, declared_sources: list[str]) -> list[Path]:
    files = set()
    for entry in declared_sources:
        candidate = (package_root / entry).resolve(strict=True)
        if not candidate.is_relative_to(package_root):
            raise ValueError(f"declared coverage source escapes contract package: {entry}")
        if candidate.is_dir():
            files.update(path for path in candidate.rglob("*.py") if path.is_file())
        elif candidate.suffix == ".py":
            files.add(candidate)
    return sorted(files)


def _normalized_coverage_lines(coverage: Coverage, package_root: Path, declared_sources: list[str]):
    data = coverage.get_data()
    lines = []
    for source in _declared_source_files(package_root, declared_sources):
        try:
            _, statements, _, _, _ = coverage.analysis2(str(source))
        except Exception as error:
            raise RuntimeError(f"cannot analyze declared source {source}: {error}") from error
        executable = sorted(set(statements))
        covered = sorted(set(data.lines(str(source)) or []).intersection(executable))
        lines.append({
            "path": source.relative_to(package_root).as_posix(),
            "executable_lines": executable,
            "covered_lines": covered,
        })
    return lines


def _coverage_percent(files) -> float:
    executable = sum(len(set(file["executable_lines"])) for file in files)
    covered = sum(len(set(file["covered_lines"]).intersection(file["executable_lines"])) for file in files)
    return covered * 100.0 / executable if executable else 0.0


def _run_supplemental_tests(test_files, root: Path, package_root: Path, coverage_sources):
    from coverage import Coverage

    coverage = Coverage(data_file=None, branch=True, config_file=False)
    errors = []
    resolved_tests = []
    for entry in test_files:
        path = (root / entry).resolve(strict=True)
        if not path.is_relative_to(package_root) or path.suffix != ".py":
            errors.append(f"invalid Python supplemental test path: {entry}")
            continue
        resolved_tests.append((entry, path))

    passed_count = 0
    if not errors:
        coverage.start()
        try:
            with contextlib.redirect_stdout(sys.stderr):
                for index, (entry, path) in enumerate(resolved_tests):
                    sys.path.insert(0, str(path.parent))
                    module_name = f"open_contract_supplemental_{index}"
                    spec = importlib.util.spec_from_file_location(module_name, path)
                    if spec is None or spec.loader is None:
                        errors.append(f"cannot load supplemental test module: {entry}")
                        continue
                    module = importlib.util.module_from_spec(spec)
                    try:
                        spec.loader.exec_module(module)
                        run_tests = getattr(module, "run", None)
                        if not callable(run_tests):
                            raise ValueError("supplemental test module must export run()")
                        outcome = run_tests()
                        if inspect.isawaitable(outcome):
                            outcome = asyncio.run(outcome)
                        if outcome is False:
                            raise ValueError("run() returned false")
                        passed_count += 1
                    except Exception as error:
                        errors.append(f"{entry}: {type(error).__name__}: {error}")
        finally:
            coverage.stop()
    lines = _normalized_coverage_lines(coverage, package_root, coverage_sources)
    data = coverage.get_data()
    native_files = {
        Path(name).resolve().relative_to(package_root).as_posix(): {
            "lines": data.lines(name) or [],
            "arcs": data.arcs(name) or [],
        }
        for name in data.measured_files()
        if Path(name).resolve().is_relative_to(package_root)
    }
    passed = not errors and passed_count == len(resolved_tests) and bool(resolved_tests)
    return ({
        "status": "passed" if passed else "failed",
        "executed": True,
        "passed": passed,
        "test_count": passed_count,
        "test_files": test_files,
        "error": "; ".join(errors) if errors else None,
        "native_report": {"format": "coverage.py", "files": native_files},
    }, lines)


def _load_validator(path: Path, root: Path):
    sys.path.insert(0, str(root))
    sys.path.insert(0, str(path.parent))
    spec = importlib.util.spec_from_file_location("open_contract_batch_validator", path)
    if spec is None or spec.loader is None:
        raise ValueError("unable to load validator module")
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    function = getattr(module, "validate", None)
    if not callable(function):
        raise ValueError("validator module must export validate(input)")
    return function


if __name__ == "__main__":
    raise SystemExit(main())
