#!/usr/bin/env python3
"""Run the dataset sandbox under llvm-cov and require full evaluator coverage."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

CRATE = Path(__file__).resolve().parent


def main() -> int:
    environment = {**os.environ}
    cargo_bin = Path.home() / ".cargo" / "bin"
    if cargo_bin.is_dir():
        environment["PATH"] = os.pathsep.join(
            [str(cargo_bin), environment.get("PATH", "")]
        )

    if (
        shutil.which("cargo", path=environment["PATH"]) is None
        or shutil.which("rustup", path=environment["PATH"]) is None
    ):
        print("rust dataset coverage requires cargo, rustup, and cargo-llvm-cov", file=sys.stderr)
        return 2
    available = subprocess.run(
        ["cargo", "llvm-cov", "--version"],
        capture_output=True,
        check=False,
        env=environment,
    )
    if available.returncode:
        print("cargo-llvm-cov is required (install with: cargo install cargo-llvm-cov --locked)", file=sys.stderr)
        return 2
    components = subprocess.run(
        ["rustup", "component", "list", "--installed"],
        capture_output=True,
        text=True,
        check=False,
        env=environment,
    )
    if components.returncode or not any(line.startswith("llvm-tools-") for line in components.stdout.splitlines()):
        print("LLVM coverage tools are required (install with: rustup component add llvm-tools-preview)", file=sys.stderr)
        return 2

    target_dir = Path(__file__).resolve().parent / "target" / "coverage"
    environment["CARGO_TARGET_DIR"] = str(target_dir)
    clean = subprocess.run(
        ["cargo", "llvm-cov", "clean", "--profraw-only", "--manifest-path", str(CRATE / "Cargo.toml")],
        cwd=CRATE,
        env=environment,
        check=False,
    )
    if clean.returncode:
        return clean.returncode
    report = target_dir / "coverage.json"
    command = [
        "cargo", "llvm-cov", "run", "--no-clean", "--manifest-path", str(CRATE / "Cargo.toml"),
        "--bin", "predicate-sandbox", "--json", "--output-path", str(report),
    ]
    completed = subprocess.run(command, cwd=CRATE, env=environment, check=False)
    if completed.returncode:
        return completed.returncode
    payload = json.loads(report.read_text(encoding="utf-8"))
    files = [file for data in payload.get("data", []) for file in data.get("files", [])]
    expected = (CRATE / "src" / "evaluate.rs").resolve()
    evaluators = [
        file for file in files
        if Path(file.get("filename", "")).resolve() == expected
    ]
    if len(evaluators) != 1:
        print("coverage report must contain exactly one Rust predicate evaluator module", file=sys.stderr)
        return 2
    summaries = evaluators[0].get("summary", {})
    metrics = ("lines", "functions", "regions")
    passed = True
    for metric in metrics:
        summary = summaries.get(metric, {})
        covered, total = summary.get("covered", 0), summary.get("count", 0)
        print(f"Rust predicate evaluator {metric}: {covered}/{total}")
        if covered != total:
            passed = False
    if not passed:
        segments = evaluators[0].get("segments", [])
        uncovered_lines = sorted({
            segment[0]
            for segment in segments
            if len(segment) >= 6 and segment[3] and segment[2] == 0 and not segment[5]
        })
        regions = evaluators[0].get("regions", [])
        uncovered_regions = sorted({
            (region[0], region[1])
            for region in regions
            if len(region) >= 8 and region[4] == 0
        })
        print(f"Uncovered evaluator lines: {uncovered_lines}", file=sys.stderr)
        print(f"Uncovered evaluator regions (line, column): {uncovered_regions}", file=sys.stderr)
    return 0 if passed else 1


if __name__ == "__main__":
    raise SystemExit(main())
