#!/usr/bin/env python3
"""Run a bootstrap contract's dataset and require dataset-only full coverage."""

from __future__ import annotations

import json
import hashlib
import os
import shutil
import subprocess
import sys
import tomllib
import tempfile
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path


def main() -> int:
    arguments = sys.argv[1:]
    update_fingerprints = False
    if arguments and arguments[0] == "--update-fingerprints":
        update_fingerprints = True
        arguments = arguments[1:]
    if len(arguments) != 1:
        print(
            "usage: check_bootstrap_contract_coverage.py [--update-fingerprints] <package>",
            file=sys.stderr,
        )
        return 2
    package = Path(arguments[0]).resolve()
    manifest = package / "Cargo.toml"
    if not manifest.is_file():
        print(f"Rust contract package not found: {manifest}", file=sys.stderr)
        return 2
    dataset_path = next(
        (package / name for name in ("dataset.json", "dataset.yaml", "dataset.yml")
         if (package / name).is_file()),
        None,
    )
    if dataset_path is None:
        print(f"dataset file not found under {package}", file=sys.stderr)
        return 2
    try:
        dataset = load_dataset(dataset_path)
    except (ValueError, OSError) as error:
        print(f"invalid dataset coverage fingerprint: {error}", file=sys.stderr)
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

    repository_root = Path(__file__).resolve().parents[2]
    artifacts = repository_root / ".coverage" / "bootstrap-contracts"
    artifacts.mkdir(parents=True, exist_ok=True)
    for stale_profile in artifacts.glob(f"{package.name}-*.profraw"):
        stale_profile.unlink(missing_ok=True)
    report = artifacts / f"{package.name}-rust.json"
    snapshots_path = artifacts / f"{package.name}-case-profiles.bin"
    binary_record = artifacts / f"{package.name}-dataset-binary.path"
    aggregate_profile = artifacts / f"{package.name}-dataset.profraw"
    indexed_profile = artifacts / f"{package.name}-dataset.profdata"
    for artifact in (report, snapshots_path, binary_record, aggregate_profile, indexed_profile):
        artifact.unlink(missing_ok=True)
    environment["OPEN_CONTRACT_BOOTSTRAP_COVERAGE_SNAPSHOTS"] = str(snapshots_path)
    environment["CARGO_TARGET_DIR"] = str(artifacts / f"{package.name}-cargo-target")
    # Keep any raw profiles emitted by Cargo or test helpers under the ignored
    # Open Contract coverage directory, never beside a bootstrap contract.
    environment["LLVM_PROFILE_FILE"] = str(artifacts / f"{package.name}-%m-%p.profraw")
    rustflags = environment.get("RUSTFLAGS", "").strip()
    environment["RUSTFLAGS"] = (
        f"{rustflags} -Cinstrument-coverage --cfg=coverage --check-cfg=cfg(coverage) "
        f"--remap-path-prefix={package}=."
    ).strip()
    dataset_test = package / "tests" / "dataset_runner.rs"
    unit_dataset_test = package / "tests" / "unit_dataset_runner.rs"
    run_dataset_binary = not unit_dataset_test.is_file() and not dataset_test.is_file()
    command = [cargo, "build" if run_dataset_binary else "test"]
    if not run_dataset_binary:
        command.append("--no-run")
    command.append("--message-format=json-render-diagnostics")
    dataset_target_name = None
    if unit_dataset_test.is_file():
        command.append("--lib")
        package_metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
        dataset_target_name = package_metadata.get("lib", {}).get(
            "name", package_metadata.get("package", {}).get("name", package.name).replace("-", "_")
        )
    elif dataset_test.is_file():
        command.extend(["--test", "dataset_runner"])
        dataset_target_name = "dataset_runner"
    else:
        package_metadata = tomllib.loads(manifest.read_text(encoding="utf-8"))
        dataset_target_name = next(
            (target.get("name") for target in package_metadata.get("bin", []) if target.get("name")),
            "dataset-contract",
        )
        command.extend(["--bin", dataset_target_name])
    command.extend(["--manifest-path", str(manifest)])
    result = subprocess.run(command, cwd=package, env=environment, check=False, capture_output=True, text=True)
    if result.returncode:
        print(result.stderr, file=sys.stderr, end="")
        return result.returncode
    binary = coverage_test_binary(result.stdout, dataset_target_name, test_profile=not run_dataset_binary)
    if not binary:
        print(f"Cargo did not report the instrumented dataset test binary for {dataset_target_name}", file=sys.stderr)
        return 2
    binary_record.write_text(
        f"{os.path.relpath(binary, package)}\n", encoding="utf-8"
    )
    environment["LLVM_PROFILE_FILE"] = str(aggregate_profile)
    arguments = []
    if unit_dataset_test.is_file():
        arguments = ["--exact", "dataset_runner_test::dataset_entries_exercise_public_api"]
    elif dataset_test.is_file():
        arguments = ["dataset_entries_exercise_public_api"]
    result = subprocess.run([str(binary), *arguments], cwd=package, env=environment, check=False)
    if result.returncode:
        return result.returncode
    if not snapshots_path.is_file():
        print("dataset harness did not emit per-case coverage snapshots", file=sys.stderr)
        return 2

    llvm_cov, llvm_profdata = llvm_tools(environment)
    source_files = sorted(
        path.relative_to(package).as_posix()
        for path in (package / "src").rglob("*.rs") if path.is_file()
    )
    if not source_files:
        print(f"no Rust implementation sources found under {package / 'src'}", file=sys.stderr)
        return 2
    merged = subprocess.run(
        [llvm_profdata, "merge", "-sparse", str(aggregate_profile), "-o", str(indexed_profile)],
        check=False, capture_output=True, text=True,
        cwd=package,
    )
    if merged.returncode:
        print(merged.stderr, file=sys.stderr, end="")
        return merged.returncode
    exported = subprocess.run(
        [
            llvm_cov,
            "export",
            f"-instr-profile={indexed_profile}",
            str(binary),
            *source_files,
        ],
        check=False, capture_output=True, text=True,
        cwd=package,
    )
    if exported.returncode:
        print(exported.stderr, file=sys.stderr, end="")
        return exported.returncode
    try:
        payload = json.loads(exported.stdout)
        report.write_text(json.dumps(payload), encoding="utf-8")
    except (OSError, ValueError) as error:
        print(f"cannot save Rust coverage report: {error}", file=sys.stderr)
        return 2
    sources = implementation_sources(payload, package)
    if not sources:
        print(f"no Rust implementation sources found under {package / 'src'}", file=sys.stderr)
        return 2
    dataset_covered = sum(source.get("summary", {}).get("lines", {}).get("covered", 0) for source in sources)
    dataset_total = sum(source.get("summary", {}).get("lines", {}).get("count", 0) for source in sources)
    dataset_regions_covered = sum(source.get("summary", {}).get("regions", {}).get("covered", 0) for source in sources)
    dataset_regions_total = sum(source.get("summary", {}).get("regions", {}).get("count", 0) for source in sources)
    dataset_functions_covered = sum(source.get("summary", {}).get("functions", {}).get("covered", 0) for source in sources)
    dataset_functions_total = sum(source.get("summary", {}).get("functions", {}).get("count", 0) for source in sources)
    dataset_instantiations_covered = sum(source.get("summary", {}).get("instantiations", {}).get("covered", 0) for source in sources)
    dataset_instantiations_total = sum(source.get("summary", {}).get("instantiations", {}).get("count", 0) for source in sources)
    print(
        f"{package.name} dataset coverage: "
        f"functions {dataset_functions_covered}/{dataset_functions_total} ({percentage(dataset_functions_covered, dataset_functions_total):.1f}%), "
        f"instantiations {dataset_instantiations_covered}/{dataset_instantiations_total} ({percentage(dataset_instantiations_covered, dataset_instantiations_total):.1f}%), "
        f"lines {dataset_covered}/{dataset_total} ({percentage(dataset_covered, dataset_total):.1f}%), "
        f"regions {dataset_regions_covered}/{dataset_regions_total} ({percentage(dataset_regions_covered, dataset_regions_total):.1f}%)"
    )
    try:
        snapshots = read_snapshots(snapshots_path)
        if [case["id"] for case in dataset["cases"]] != [case_id for case_id, _ in snapshots]:
            print("dataset coverage snapshots do not match dataset case order", file=sys.stderr)
            return 2
        case_fingerprints = calculate_case_fingerprints(
            snapshots, binary, package, llvm_cov, llvm_profdata, source_files, artifacts
        )
    except (OSError, ValueError, RuntimeError) as error:
        print(f"cannot calculate per-case coverage fingerprints: {error}", file=sys.stderr)
        return 2
    case_mismatches = []
    debug_selection = os.environ.get("OPEN_CONTRACT_DEBUG_FINGERPRINT_CASE", "").strip()
    debug_all_mismatches = debug_selection.lower() in {"1", "true", "yes", "on"}
    debug_case = None if not debug_selection or debug_all_mismatches else debug_selection
    for case, (case_id, actual, inputs) in zip(dataset["cases"], case_fingerprints):
        expected = declared_fingerprint(case)
        mismatch = fingerprint_mismatch(case, actual)
        if case_id == debug_case or (debug_all_mismatches and mismatch):
            print(f"Fingerprint inputs for case {case_id} (observed {actual}):")
            print(json.dumps(inputs, separators=(",", ":")))
        if mismatch:
            case_mismatches.append(f"{case_id}: expected {expected}, observed {actual}")
    if update_fingerprints:
        changed = []
        for case, (_, actual, _) in zip(dataset["cases"], case_fingerprints):
            old = declared_fingerprint(case)
            if old != actual:
                changed.append((case["id"], old, actual))
            case["coverage_fingerprint"] = actual
        if dataset_path.suffix == ".json":
            serialized = json.dumps(dataset, indent=2, ensure_ascii=False) + "\n"
        else:
            try:
                import yaml
            except ImportError as error:
                print(f"PyYAML is required to update YAML datasets: {error}", file=sys.stderr)
                return 2
            serialized = yaml.safe_dump(dataset, sort_keys=False, allow_unicode=True)
        dataset_path.write_text(serialized, encoding="utf-8")
        print(f"Updated {len(changed)} changed coverage fingerprints in {dataset_path}")
        for case_id, old, actual in changed:
            print(f"  {case_id}: {old} -> {actual}")
        case_mismatches = []
    if case_mismatches:
        print("bootstrap dataset coverage fingerprint mismatches:", file=sys.stderr)
        for mismatch in case_mismatches:
            print(f"  {mismatch}", file=sys.stderr)
        return 1
    print(f"Rust coverage report: {report}")
    metrics = (
        (dataset_functions_covered, dataset_functions_total),
        (dataset_instantiations_covered, dataset_instantiations_total),
        (dataset_covered, dataset_total),
        (dataset_regions_covered, dataset_regions_total),
    )
    complete = all(total > 0 and covered == total for covered, total in metrics)
    if not complete:
        print(
            "bootstrap dataset coverage must reach 100% for functions, "
            "instantiations, lines, and regions",
            file=sys.stderr,
        )
    return 0 if complete else 1


def percentage(covered: int, total: int) -> float:
    return covered * 100.0 / total if total else 0.0


def load_dataset(path: Path) -> dict:
    if path.suffix == ".json":
        dataset = json.loads(path.read_text(encoding="utf-8"))
    else:
        try:
            import yaml
        except ImportError as error:
            raise ValueError("PyYAML is required to read YAML bootstrap datasets") from error
        dataset = yaml.safe_load(path.read_text(encoding="utf-8"))
    if not isinstance(dataset, dict) or not isinstance(dataset.get("cases"), list):
        raise ValueError("dataset must contain a cases array")
    for case in dataset["cases"]:
        if not isinstance(case, dict) or not isinstance(case.get("id"), str):
            raise ValueError("every dataset case must have a string id")
    return dataset


def coverage_fingerprint(sources: list[dict], package: Path) -> str:
    inputs = coverage_fingerprint_inputs(sources, package)
    if not inputs:
        return "0" * 32
    encoded = json.dumps(inputs, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()[:32]


def coverage_fingerprint_inputs(sources: list[dict], package: Path) -> list[dict]:
    files = []
    for source in sources:
        covered = sorted({
            segment[0]
            for segment in source.get("segments", [])
            if len(segment) >= 4 and segment[2] > 0 and segment[3]
        })
        if covered:
            filename = resolve_source_path(source["filename"], package)
            relative = filename.relative_to(package).as_posix()
            source_lines = filename.read_text(encoding="utf-8").splitlines()
            if any(number < 1 or number > len(source_lines) for number in covered):
                raise ValueError(f"coverage line is outside source file {relative}")
            lines = [source_lines[number - 1] for number in covered]
            files.append({"path": relative, "lines": lines})
    files.sort(key=lambda item: item["path"])
    return files


def implementation_sources(payload: dict, package: Path) -> list[dict]:
    files = [item for data in payload.get("data", []) for item in data.get("files", [])]
    source_root = (package / "src").resolve()
    sources = []
    for item in files:
        filename = item.get("filename", "")
        if not filename.endswith(".rs"):
            continue
        try:
            source_path = resolve_source_path(filename, package)
            source_path.relative_to(source_root)
        except (OSError, ValueError):
            continue
        sources.append(item)
    return sources


def resolve_source_path(filename: str, package: Path) -> Path:
    path = Path(filename)
    if not path.is_absolute():
        path = package / path
    return path.resolve()


def declared_fingerprint(case: dict) -> str:
    value = case.get("coverage_fingerprint")
    if (
        isinstance(value, str)
        and len(value) == 32
        and all(char in "0123456789abcdef" for char in value)
    ):
        return value
    return "0" * 32


def fingerprint_mismatch(case: dict, actual: str) -> bool:
    declared = case.get("coverage_fingerprint")
    pin_is_missing_or_zero = (
        not isinstance(declared, str)
        or len(declared) != 32
        or any(char not in "0123456789abcdef" for char in declared)
        or declared == "0" * 32
    )
    return pin_is_missing_or_zero or declared != actual


def read_snapshots(path: Path) -> list[tuple[str, bytes]]:
    data = path.read_bytes()
    magic = b"OCOVCASE"
    if not data.startswith(magic) or len(data) < len(magic) + 4:
        raise ValueError("invalid snapshot header")
    offset = len(magic)
    count = int.from_bytes(data[offset:offset + 4], "little")
    offset += 4
    snapshots = []
    for _ in range(count):
        if offset + 12 > len(data):
            raise ValueError("truncated snapshot record header")
        id_length = int.from_bytes(data[offset:offset + 4], "little")
        profile_length = int.from_bytes(data[offset + 4:offset + 12], "little")
        offset += 12
        end_id = offset + id_length
        end_profile = end_id + profile_length
        if end_profile > len(data):
            raise ValueError("truncated snapshot record")
        case_id = data[offset:end_id].decode("utf-8")
        snapshots.append((case_id, data[end_id:end_profile]))
        offset = end_profile
    if offset != len(data):
        raise ValueError("trailing data after snapshot records")
    return snapshots


def calculate_case_fingerprints(
    snapshots: list[tuple[str, bytes]],
    binary: Path,
    package: Path,
    llvm_cov: str,
    llvm_profdata: str,
    source_files: list[str],
    artifacts: Path,
) -> list[tuple[str, str, list[dict]]]:
    workers = coverage_worker_count()
    print(f"Per-case LLVM coverage workers: {workers}")

    def fingerprint(snapshot: tuple[str, bytes]) -> tuple[str, str, list[dict]]:
        case_id, profile_bytes = snapshot
        with tempfile.TemporaryDirectory(
            prefix="bootstrap-case-coverage-", dir=artifacts
        ) as temporary_directory:
            temporary = Path(temporary_directory)
            raw_profile = temporary / "case.profraw"
            indexed_profile = temporary / "case.profdata"
            raw_profile.write_bytes(profile_bytes)
            merged = subprocess.run(
                [llvm_profdata, "merge", "-sparse", str(raw_profile), "-o", str(indexed_profile)],
                check=False, capture_output=True, text=True,
            )
            if merged.returncode:
                detail = merged.stderr.strip() or merged.stdout.strip()
                raise RuntimeError(f"case {case_id}: llvm-profdata failed: {detail}")
            exported = subprocess.run(
                [
                    llvm_cov,
                    "export",
                    "--skip-functions",
                    "--skip-expansions",
                    f"-instr-profile={indexed_profile}",
                    str(binary),
                    *source_files,
                ],
                check=False, capture_output=True, text=True, cwd=package,
            )
            if exported.returncode:
                detail = exported.stderr.strip() or exported.stdout.strip()
                raise RuntimeError(f"case {case_id}: llvm-cov failed: {detail}")
            try:
                payload = json.loads(exported.stdout)
            except ValueError as error:
                raise RuntimeError(f"case {case_id}: llvm-cov returned invalid JSON: {error}") from error
            sources = implementation_sources(payload, package)
            inputs = coverage_fingerprint_inputs(sources, package)
            encoded = json.dumps(inputs, separators=(",", ":"), ensure_ascii=False).encode("utf-8")
            value = hashlib.sha256(encoded).hexdigest()[:32] if inputs else "0" * 32
            return case_id, value, inputs

    with ThreadPoolExecutor(max_workers=workers) as executor:
        return list(executor.map(fingerprint, snapshots))


def coverage_worker_count() -> int:
    configured = os.environ.get("OPEN_CONTRACT_COVERAGE_WORKERS")
    if configured is None:
        return min(os.cpu_count() or 1, 4)
    try:
        workers = int(configured)
    except ValueError as error:
        raise RuntimeError("OPEN_CONTRACT_COVERAGE_WORKERS must be a positive integer") from error
    if workers < 1:
        raise RuntimeError("OPEN_CONTRACT_COVERAGE_WORKERS must be a positive integer")
    return workers


def llvm_tools(environment: dict[str, str]) -> tuple[str, str]:
    rustc = shutil.which("rustc", path=environment.get("PATH"))
    if not rustc:
        raise RuntimeError("rustc is required to locate LLVM coverage tools")
    host_result = subprocess.run([rustc, "-vV"], check=True, capture_output=True, text=True)
    host = next(line.split(": ", 1)[1] for line in host_result.stdout.splitlines() if line.startswith("host: "))
    sysroot_result = subprocess.run([rustc, "--print", "sysroot"], check=True, capture_output=True, text=True)
    tools = Path(sysroot_result.stdout.strip()) / "lib" / "rustlib" / host / "bin"
    llvm_cov = tools / "llvm-cov"
    llvm_profdata = tools / "llvm-profdata"
    if not llvm_cov.is_file() or not llvm_profdata.is_file():
        raise RuntimeError(f"Rust LLVM tools are unavailable under {tools}")
    return str(llvm_cov), str(llvm_profdata)


def coverage_test_binary(
    cargo_output: str, target_name: str, *, test_profile: bool = True
) -> Path | None:
    """Use Cargo's artifact record, never a newest-binary guess in a shared target dir."""
    for line in cargo_output.splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        if message.get("reason") != "compiler-artifact":
            continue
        target = message.get("target", {})
        profile = message.get("profile", {})
        executable = message.get("executable")
        if (
            target.get("name") == target_name
            and profile.get("test") is test_profile
            and isinstance(executable, str)
        ):
            path = Path(executable)
            if path.is_file() and os.access(path, os.X_OK):
                return path
    return None


if __name__ == "__main__":
    raise SystemExit(main())
