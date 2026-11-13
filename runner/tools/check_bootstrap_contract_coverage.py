#!/usr/bin/env python3
"""Run one bootstrap Rust contract's dataset executable with coverage."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check_coverage.py <contract-package-dir>", file=sys.stderr)
        return 2
    package = Path(sys.argv[1]).resolve()
    manifest = package / "Cargo.toml"
    if not manifest.is_file():
        print(f"Rust contract package not found: {manifest}", file=sys.stderr)
        return 2

    environment = dict(os.environ)
    cargo_bin = Path.home() / ".cargo" / "bin"
    if cargo_bin.is_dir():
        environment["PATH"] = os.pathsep.join(
            [str(cargo_bin), environment.get("PATH", "")]
        )
    cargo = shutil.which("cargo", path=environment.get("PATH"))
    if not cargo:
        print("Rust contract coverage requires cargo", file=sys.stderr)
        return 2

    artifacts = Path.home() / ".cache" / "open-contract-coverage"
    artifacts.mkdir(parents=True, exist_ok=True)
    report = artifacts / f"{package.name}-rust.json"
    environment["CARGO_TARGET_DIR"] = str(
        Path.home() / ".cache" / "open-contract-target" / f"bootstrap-{package.name}"
    )
    supplemental_tests = read_supplemental_tests(package)
    if supplemental_tests is None:
        return 2
    command = [
        cargo,
        "llvm-cov",
        "run",
        "--bin",
        "dataset-contract",
        "--manifest-path",
        str(manifest),
        "--json",
        "--output-path",
        str(report),
    ]
    clean = subprocess.run(
        [
            cargo,
            "llvm-cov",
            "clean",
            "--profraw-only",
            "--manifest-path",
            str(manifest),
        ],
        cwd=package,
        env=environment,
        check=False,
    )
    if clean.returncode:
        return clean.returncode
    result = subprocess.run(command, cwd=package, env=environment, check=False)
    if result.returncode:
        return result.returncode

    payload = json.loads(report.read_text(encoding="utf-8"))
    sources = implementation_sources(payload, package, supplemental_tests)
    if not sources:
        print(f"no Rust implementation sources found under {package / 'src'}", file=sys.stderr)
        return 2
    dataset_lcov = run_lcov_report(cargo, manifest, package, environment)
    if dataset_lcov is None:
        return 2
    dataset_lines = parse_lcov_lines(dataset_lcov, sources)
    dataset_covered, dataset_total = combined_totals(dataset_lines, dataset_lines)
    line_covered, line_total = dataset_covered, dataset_total
    combined_lines = dataset_lines
    print(f"{package.name} dataset-only Rust lines: {line_covered}/{line_total}")
    report_path = report
    if supplemental_tests:
        if line_total == 0 or line_covered / line_total < 0.90:
            print("Supplemental tests skipped: dataset-only coverage is below 90%")
            print_uncovered(package, report_path, dataset_lines, dataset_lines)
            return 1
        for index, test_file in enumerate(supplemental_tests):
            test_report = artifacts / f"{package.name}-supplemental-{index}.json"
            test_filter = f"{test_file.stem}_tests::"
            test_command = [
                cargo,
                "llvm-cov",
                "test",
                "--manifest-path",
                str(manifest),
                "--lib",
                "--no-clean",
                "--json",
                "--output-path",
                str(test_report),
                test_filter,
            ]
            test_result = subprocess.run(
                test_command,
                cwd=package,
                env=environment,
                check=False,
                capture_output=True,
                text=True,
            )
            if test_result.returncode:
                print(test_result.stdout, end="")
                print(test_result.stderr, file=sys.stderr, end="")
                return test_result.returncode
            if "running 0 tests" in test_result.stdout:
                print(f"declared supplemental test file ran no tests: {test_file}", file=sys.stderr)
                return 2
            report_path = test_report
            # Keep the denominator tied to lines executable in the dataset build;
            # cfg(test) code from the supplemental build must not enter it.
        combined_lcov = run_lcov_report(cargo, manifest, package, environment)
        if combined_lcov is None:
            return 2
        combined_lines = parse_lcov_lines(combined_lcov, sources)
        line_covered, line_total = combined_totals(dataset_lines, combined_lines)
        added_lines = max(0, line_covered - dataset_covered)
        print(f"{package.name} supplemental tests added {added_lines} implementation lines")
        print(f"{package.name} combined Rust lines: {line_covered}/{line_total}")
    combined_complete = line_total > 0 and line_covered == line_total
    print_uncovered(package, report_path, dataset_lines, combined_lines)
    return 0 if combined_complete else 1


def read_supplemental_tests(package: Path) -> list[Path] | None:
    manifest_path = package / "contract.yaml"
    try:
        import yaml
    except ImportError:
        print("supplemental tests require PyYAML to read contract.yaml", file=sys.stderr)
        return None
    try:
        document = yaml.safe_load(manifest_path.read_text(encoding="utf-8")) or {}
    except (OSError, yaml.YAMLError) as error:
        print(f"cannot read contract supplemental tests: {error}", file=sys.stderr)
        return None
    entries = document.get("files", {}).get("supplemental_tests", [])
    if not isinstance(entries, list) or any(not isinstance(item, str) for item in entries):
        print("files.supplemental_tests must be a list of package-relative paths", file=sys.stderr)
        return None
    tests = []
    for entry in entries:
        path = (package / entry).resolve()
        if not path.is_relative_to(package) or not path.is_file():
            print(f"invalid supplemental test file: {entry}", file=sys.stderr)
            return None
        tests.append(path)
    return tests


def implementation_sources(payload: dict, package: Path, supplemental_tests: list[Path]) -> list[dict]:
    files = [item for data in payload.get("data", []) for item in data.get("files", [])]
    source_root = (package / "src").as_posix().rstrip("/") + "/"
    ignored = {path.resolve() for path in supplemental_tests}
    return [
        item for item in files
        if item.get("filename", "").replace("\\", "/").startswith(source_root)
        and item.get("filename", "").replace("\\", "/").endswith(".rs")
        and not item.get("filename", "").replace("\\", "/").endswith("/bin/dataset-contract.rs")
        and Path(item.get("filename", "")).resolve() not in ignored
    ]


def run_lcov_report(
    cargo: str, manifest: Path, package: Path, environment: dict[str, str]
) -> str | None:
    result = subprocess.run(
        [cargo, "llvm-cov", "report", "--lcov", "--manifest-path", str(manifest)],
        cwd=package,
        env=environment,
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        print(result.stderr, file=sys.stderr, end="")
        return None
    return result.stdout


def parse_lcov_lines(contents: str, sources: list[dict]) -> dict[Path, tuple[set[int], set[int]]]:
    """Return actionable executable lines, excluding brace-only source lines."""
    allowed = {Path(source["filename"]).resolve() for source in sources}
    source_lines = {}
    for filename in allowed:
        try:
            source_lines[filename] = filename.read_text(encoding="utf-8").splitlines()
        except OSError:
            source_lines[filename] = []
    result: dict[Path, tuple[set[int], set[int]]] = {}
    current: Path | None = None
    for line in contents.splitlines():
        if line.startswith("SF:"):
            candidate = Path(line[3:]).resolve()
            current = candidate if candidate in allowed else None
        elif current is not None and line.startswith("DA:"):
            fields = line[3:].split(",")
            if len(fields) < 2:
                continue
            executable, covered = result.setdefault(current, (set(), set()))
            number, hits = int(fields[0]), int(fields[1])
            lines = source_lines.get(current, [])
            if 0 < number <= len(lines) and lines[number - 1].strip() in {"}", "};"}:
                continue
            executable.add(number)
            if hits > 0:
                covered.add(number)
    return result


def combined_totals(
    dataset: dict[Path, tuple[set[int], set[int]]],
    combined: dict[Path, tuple[set[int], set[int]]],
) -> tuple[int, int]:
    """Count test-covered lines only where the dataset profile marks code executable."""
    total = 0
    covered = 0
    for filename, (executable, dataset_covered) in dataset.items():
        total += len(executable)
        combined_covered = combined.get(filename, (set(), set()))[1]
        covered += len(dataset_covered | (combined_covered & executable))
    return covered, total


def print_uncovered(
    package: Path,
    report_path: Path,
    dataset: dict[Path, tuple[set[int], set[int]]],
    combined: dict[Path, tuple[set[int], set[int]]],
) -> None:
    missing_by_file = {}
    for filename, (executable, dataset_covered) in dataset.items():
        combined_covered = combined.get(filename, (set(), set()))[1]
        missing = sorted(executable - dataset_covered - combined_covered)
        if missing:
            missing_by_file[filename] = missing
    if missing_by_file:
        print("Uncovered Rust implementation lines:")
        for filename, missing in sorted(missing_by_file.items()):
            print(f"  {filename.relative_to(package)}: {format_line_ranges(missing)}")
    print(f"Rust coverage report: {report_path}")


def format_line_ranges(lines: list[int]) -> str:
    """Render sorted line numbers as compact, inclusive ranges."""
    if not lines:
        return ""
    ranges = []
    start = previous = lines[0]
    for line in lines[1:]:
        if line == previous + 1:
            previous = line
            continue
        ranges.append(str(start) if start == previous else f"{start}-{previous}")
        start = previous = line
    ranges.append(str(start) if start == previous else f"{start}-{previous}")
    return ", ".join(ranges)


if __name__ == "__main__":
    raise SystemExit(main())
