from __future__ import annotations

from pathlib import Path

import pytest
import yaml
import pytest

from open_contracts_runner.cli import _execute_manifest, main
from open_contracts_runner.manifest import read_manifest, stage_package


def make_package(root: Path, name: str = "sample", value: int = 3) -> Path:
    root.mkdir(parents=True, exist_ok=True)
    (root / "data.yaml").write_text(
        yaml.safe_dump({"version": 1, "cases": [{"id": "case", "input": value, "expected": value + 1}]}),
        encoding="utf-8",
    )
    (root / "check.py").write_text("def validate(value):\n    return value + 1\n", encoding="utf-8")
    manifest = {
        "version": "1", "name": name,
        "files": {"dataset": "data.yaml", "validator": "check.py", "support": [], "nested_contracts": []},
        "filesystem": {"read": ["data.yaml", "check.py"], "write": []},
    }
    path = root / "contract.yaml"
    path.write_text(yaml.safe_dump(manifest), encoding="utf-8")
    return path


def test_manifest_stages_only_declared_files(tmp_path: Path) -> None:
    manifest_path = make_package(tmp_path / "package")
    (tmp_path / "package" / "private.txt").write_text("secret", encoding="utf-8")
    manifest, source_root = read_manifest(manifest_path)
    with stage_package(manifest, source_root) as staged:
        assert (Path(staged) / "data.yaml").is_file()
        assert not (Path(staged) / "private.txt").exists()


def test_manifest_rejects_traversal_and_symlink_escape(tmp_path: Path) -> None:
    manifest_path = make_package(tmp_path / "package")
    value = yaml.safe_load(manifest_path.read_text(encoding="utf-8"))
    value["files"]["dataset"] = "../outside.yaml"
    manifest_path.write_text(yaml.safe_dump(value), encoding="utf-8")
    with pytest.raises(ValueError, match="stay within the package"):
        read_manifest(manifest_path)

    manifest_path = make_package(tmp_path / "package")
    outside = tmp_path / "outside.py"
    outside.write_text("def validate(value): return value\n", encoding="utf-8")
    (tmp_path / "package" / "check.py").unlink()
    (tmp_path / "package" / "check.py").symlink_to(outside)
    with pytest.raises(ValueError, match="symlink"):
        read_manifest(manifest_path)


@pytest.mark.parametrize("backend", ["none", "landlock"])
def test_parent_runs_nested_contract_and_collects_both_case_results(tmp_path: Path, backend: str) -> None:
    parent = make_package(tmp_path / "parent", "parent", value=3)
    child = make_package(tmp_path / "parent" / "child", "child", value=7)
    manifest = yaml.safe_load(parent.read_text(encoding="utf-8"))
    manifest["files"]["nested_contracts"] = ["child/contract.yaml"]
    manifest["filesystem"]["read"] = ["data.yaml", "check.py", "child"]
    parent.write_text(yaml.safe_dump(manifest), encoding="utf-8")

    report = _execute_manifest(parent, backend, set())

    assert (report.passed, report.total) == (2, 2)
    assert {result.target_id for result in report.results} == {"default", "parent/child/default"}

    standalone_child = _execute_manifest(child, backend, set())
    assert (standalone_child.passed, standalone_child.total) == (1, 1)


def test_parent_coverage_remaps_nested_validator_paths(tmp_path: Path, capsys) -> None:
    parent = make_package(tmp_path / "parent", "parent", value=3)
    child = make_package(tmp_path / "parent" / "child", "child", value=7)
    manifest = yaml.safe_load(parent.read_text(encoding="utf-8"))
    manifest["files"]["nested_contracts"] = ["child/contract.yaml"]
    manifest["filesystem"]["read"] = ["data.yaml", "check.py", "child"]
    parent.write_text(yaml.safe_dump(manifest), encoding="utf-8")

    result = main([
        "run-manifest", str(parent), "--isolation", "landlock", "--coverage",
        "--coverage-source", str(parent.parent / "check.py"),
        "--coverage-source", str(child.parent / "check.py"),
        "--coverage-file", str(tmp_path / ".coverage"), "--coverage-threshold", "0", "--quiet",
    ])

    assert result == 0
    report = capsys.readouterr().err
    assert str(parent.parent / "check.py") in report
    assert str(child.parent / "check.py") in report
