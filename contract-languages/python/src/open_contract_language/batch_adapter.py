"""Execute one whole validator batch and return outputs plus native coverage."""

from __future__ import annotations

import asyncio
import contextlib
from concurrent.futures import ThreadPoolExecutor
import importlib.util
import inspect
import json
import sys
from pathlib import Path


def run_dataset(request, validator_loader=None):
    """Run one batch with the supplied contract validator and return raw data.

    A contract entrypoint may pass a loader that imports its validator. The
    loader runs after coverage starts so validator module execution is measured.
    """
    try:
        root = Path.cwd().resolve(strict=True)
        validator = request.get("validator")
        if validator is not None:
            validator = Path(validator).resolve(strict=True)
        if validator is not None and not validator.is_relative_to(root):
            raise ValueError("validator path escaped staged package root")
        if validator is None and validator_loader is None:
            raise ValueError("validator_loader is required when request.validator is omitted")
        package_root = (root / request.get("package", "")).resolve(strict=True)
        if not package_root.is_relative_to(root):
            raise ValueError("contract package path escaped staged root")
        coverage_sources = request.get("coverage_sources", [])
        declared_sources = _declared_source_files(package_root, coverage_sources)
    except Exception as exc:
        return {"results": [], "coverage": None, "error": f"{type(exc).__name__}: {exc}"}

    try:
        from coverage import Coverage
    except ImportError as exc:
        return {"results": [], "coverage": None, "error": f"coverage unavailable: {exc}"}

    coverage = Coverage(
        data_file=None,
        branch=True,
        config_file=False,
        concurrency=["thread"],
        include=[str(source) for source in declared_sources]
        or [str(package_root / ".open-contract-no-public-sources")],
    )
    collect_fingerprints = request.get("coverage_fingerprints", True) is not False
    results = []
    keys_by_context = {}
    setup_context = "open-contract:setup"
    coverage.start()
    try:
        with contextlib.redirect_stdout(sys.stderr):
            coverage.switch_context(setup_context)
            try:
                sys.path.insert(0, str(root))
                if validator is not None:
                    sys.path.insert(0, str(validator.parent))
                loaded = validator_loader() if validator_loader else _load_validator(validator, root)
                executor = loaded if callable(loaded) else getattr(loaded, "validate", None)
                if not callable(executor):
                    raise TypeError("validator loader must return validate(input, context)")
            except Exception as exc:
                for item in request.get("requests", []):
                    results.append({"key": item["key"], "error": f"{type(exc).__name__}: {exc}"})
            else:
                requests = request.get("requests", [])
                index = 0
                while index < len(requests):
                    first = requests[index]
                    group = [first]
                    if first.get("parallel") is True:
                        case_id = first["key"]["case_id"]
                        for candidate in requests[index + 1:]:
                            if (candidate["key"].get("dataset_id") != first["key"].get("dataset_id")
                                    or candidate["key"]["case_id"] != case_id
                                    or candidate.get("parallel") is not True):
                                break
                            group.append(candidate)
                    context = _coverage_context([item["key"] for item in group])
                    if collect_fingerprints:
                        keys_by_context[context] = [item["key"] for item in group]
                    if len(group) > 1:
                        with ThreadPoolExecutor() as pool:
                            results.extend(pool.map(
                                lambda item: _execute_request(executor, item, coverage, context), group
                            ))
                    else:
                        results.append(_execute_request(executor, first, coverage, context))
                    index += len(group)
    finally:
        coverage.stop()

    data = coverage.get_data()
    files = {}
    contexts_by_file = {}
    for source in declared_sources:
        name = str(source)
        relative = source.relative_to(package_root).as_posix()
        files[relative] = {"lines": data.lines(name) or [], "arcs": data.arcs(name) or []}
        contexts_by_file[name] = data.contexts_by_lineno(name)
    coverage_lines = _normalized_coverage_lines(
        coverage, package_root, coverage_sources
    )
    coverage_metrics = _normalized_coverage_metrics(
        coverage, package_root, coverage_sources, coverage_lines
    )
    native = {"format": "coverage.py", "sources": sorted(files), "files": files}
    scopes = []
    if collect_fingerprints:
        for context, keys in keys_by_context.items():
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
            scopes.extend({"key": key, "files": scoped_files} for key in keys)
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
    return {
        "results": results,
        "coverage": native,
        "coverage_scopes": scopes,
        "coverage_lines": sorted(coverage_lines, key=lambda item: item["path"]),
        "coverage_metrics": coverage_metrics,
        "additional_test_coverage": additional_report,
        "additional_coverage_lines": additional_coverage_lines,
    }


def _coverage_context(keys):
    return "open-contract:" + json.dumps(keys, sort_keys=True, separators=(",", ":"))


def _execute_request(executor, item, coverage, coverage_context):
    context = {
        "case_id": item["key"]["case_id"],
        "step_id": item["key"].get("step_id"),
        "setup": item.get("setup"),
        "parallel": item.get("parallel", False),
    }
    try:
        coverage.switch_context(coverage_context)
        output = executor(item["input"], context)
        if inspect.isawaitable(output):
            output = asyncio.run(output)
        output = json.loads(json.dumps(output, ensure_ascii=False, allow_nan=False))
        return {"key": item["key"], "output": output}
    except Exception as exc:
        return {"key": item["key"], "error": f"{type(exc).__name__}: {exc}"}


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


def _normalized_coverage_metrics(coverage: Coverage, package_root: Path, declared_sources: list[str], coverage_lines):
    metrics = {}

    def add(name, path, covered, total):
        metric = metrics.setdefault(name, {"covered": 0, "total": 0, "files": []})
        metric["covered"] += covered
        metric["total"] += total
        metric["files"].append({"path": path, "covered": covered, "total": total})

    for file in coverage_lines:
        add(
            "line",
            file["path"],
            len(file["covered_lines"]),
            len(file["executable_lines"]),
        )
    for source in _declared_source_files(package_root, declared_sources):
        try:
            analysis = coverage._analyze(str(source))
        except Exception as error:
            raise RuntimeError(f"cannot calculate metrics for declared source {source}: {error}") from error
        path = source.relative_to(package_root).as_posix()
        numbers = analysis.numbers
        if numbers.n_statements:
            add("statement", path, numbers.n_executed, numbers.n_statements)
        if numbers.n_branches:
            add("branch", path, numbers.n_executed_branches, numbers.n_branches)
    for metric in metrics.values():
        metric["files"].sort(key=lambda item: item["path"])
    return metrics


def _run_supplemental_tests(test_files, root: Path, package_root: Path, coverage_sources):
    from coverage import Coverage

    declared_sources = _declared_source_files(package_root, coverage_sources)
    coverage = Coverage(
        data_file=None,
        branch=True,
        config_file=False,
        include=[str(source) for source in declared_sources]
        or [str(package_root / ".open-contract-no-public-sources")],
    )
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
        source.relative_to(package_root).as_posix(): {
            "lines": data.lines(str(source)) or [],
            "arcs": data.arcs(str(source)) or [],
        }
        for source in declared_sources
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
        raise ValueError("validator module must export validate(input, context)")
    return function
