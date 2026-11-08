from __future__ import annotations

from argparse import Namespace
from pathlib import Path
import sys
from types import SimpleNamespace

import pytest

from open_contracts_runner.isolation import (
    BubblewrapIsolationError,
    _build_command,
    _rewrite_child_arguments,
    _pythonpath,
    run_with_bubblewrap,
)


def test_bubblewrap_mounts_runner_contract_and_declared_dependencies_read_only(
    tmp_path: Path,
) -> None:
    contracts = tmp_path / "contracts"
    contracts.mkdir()
    runner = tmp_path / "open-contract"
    runner.mkdir()
    dependency = tmp_path / "dependency"
    dependency.mkdir()
    output = tmp_path / "output"
    output.mkdir()
    sandbox_root = Path("/contract-run-unique")

    command, environment = _build_command(
        bwrap="/usr/bin/bwrap",
        contract_root=contracts,
        runner_root=runner,
        dependency_roots=[dependency],
        pythonpath=[str(sandbox_root), "/open-contract/contract-runner/python/src", "/dependencies/0"],
        sandbox_root=sandbox_root,
        child_arguments=["run", str(sandbox_root / "contracts/example/dataset.json")],
        output_directory=output,
    )

    assert environment == {}
    assert command[0:5] == [
        "/usr/bin/bwrap", "--die-with-parent", "--new-session", "--unshare-all", "--clearenv"
    ]
    assert [str(contracts), str(sandbox_root / "contracts")] == _pair_after(command, "--ro-bind", str(contracts))
    assert [str(runner), "/open-contract"] == _pair_after(command, "--ro-bind", str(runner))
    assert [str(dependency), "/dependencies/0"] == _pair_after(command, "--ro-bind", str(dependency))
    assert [str(output), "/outputs"] == _pair_after(command, "--bind", str(output))
    assert "--overlay" not in command
    assert "/" not in command[1:]
    expected_tail = [
        "--chdir", str(sandbox_root / "work"), "--", sys.executable,
        "-m", "open_contracts_runner", "run", str(sandbox_root / "contracts/example/dataset.json"),
    ]
    assert command[-len(expected_tail):] == expected_tail


def test_rewrites_dataset_and_coverage_paths_into_sandbox_roots(tmp_path: Path) -> None:
    contracts = tmp_path / "contracts"
    runner = tmp_path / "open-contract"
    dependency = tmp_path / "dependency"
    dataset = contracts / "example" / "dataset.json"
    source = contracts / "example" / "api.py"
    sandbox_root = Path("/contract-run-specific")

    rewritten = _rewrite_child_arguments(
        ["run", str(dataset), "--coverage-file", "/tmp/report/.coverage",
         "--coverage-source", str(source)],
        Path("example/dataset.json"),
        contracts,
        runner,
        [dependency],
        sandbox_root,
    )

    assert rewritten == [
        "run", "/contract-run-specific/contracts/example/dataset.json",
        "--coverage-file", "/outputs/.coverage",
        "--coverage-source", "/contract-run-specific/contracts/example/api.py",
    ]


def test_maps_repository_pythonpath_to_random_root_for_contracts_package(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    repository = tmp_path / "repository"
    contracts = repository / "contracts"
    contracts.mkdir(parents=True)
    runner = tmp_path / "open-contract"
    runner.mkdir()
    sandbox_root = Path("/contract-run-specific")
    monkeypatch.setenv("PYTHONPATH", str(repository))

    assert _pythonpath(contracts, runner, [], sandbox_root)[0] == str(sandbox_root)


def test_bubblewrap_requires_contract_root(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setattr("open_contracts_runner.isolation.shutil.which", lambda _: "/usr/bin/bwrap")
    args = Namespace(
        command="run", contract_root=None, sandbox_dependency=[], dataset=tmp_path / "data.json",
        coverage_file=tmp_path / ".coverage", coverage_append=False, coverage=False,
    )

    with pytest.raises(BubblewrapIsolationError, match="--contract-root is required"):
        run_with_bubblewrap(args, ["run", str(args.dataset)])


def test_run_with_bubblewrap_uses_random_contract_mount_and_returns_coverage(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch,
) -> None:
    contracts = tmp_path / "contracts"
    contracts.mkdir()
    dataset = contracts / "dataset.json"
    dataset.write_text("{}", encoding="utf-8")
    runner = tmp_path / "open-contract"
    runner.mkdir()
    (runner / "contract-runner/python/src/open_contracts_runner").mkdir(parents=True)
    coverage_file = tmp_path / "coverage" / ".coverage"
    args = Namespace(
        command="run", contract_root=contracts, sandbox_dependency=[], dataset=dataset,
        coverage_file=coverage_file, coverage_append=False, coverage=True,
        coverage_threshold=100,
    )
    monkeypatch.setattr("open_contracts_runner.isolation.shutil.which", lambda _: "/usr/bin/bwrap")
    monkeypatch.setattr("open_contracts_runner.isolation._find_runner_root", lambda: runner)
    monkeypatch.setenv("PYTHONPATH", str(tmp_path))
    seen: dict[str, list[str]] = {}

    def fake_run(command: list[str], env: dict[str, str], check: bool) -> SimpleNamespace:
        assert env == {}
        assert check is False
        seen["command"] = command
        output_mount = next(
            command[index + 1:index + 3]
            for index, value in enumerate(command[:-2])
            if value == "--bind" and command[index + 2] == "/outputs"
        )
        output_dir = Path(output_mount[0])
        (output_dir / ".coverage").write_text("coverage", encoding="utf-8")
        return SimpleNamespace(returncode=0)

    monkeypatch.setattr("open_contracts_runner.isolation.subprocess.run", fake_run)

    result = run_with_bubblewrap(args, ["run", str(dataset), "--coverage"])

    assert result == 0
    assert coverage_file.read_text(encoding="utf-8") == "coverage"
    contract_mount = _pair_after(seen["command"], "--ro-bind", str(contracts))[1]
    assert contract_mount.startswith("/contract-run-")
    assert "/workspace" not in seen["command"]
    assert _pythonpath(contracts, runner, [], Path(contract_mount).parent)[:2] == [
        str(Path(contract_mount).parent), "/open-contract/contract-runner/python/src"
    ]


def _pair_after(command: list[str], option: str, source: str) -> list[str]:
    for index, value in enumerate(command[:-2]):
        if value == option and command[index + 1] == source:
            return command[index + 1:index + 3]
    raise AssertionError(f"{option} {source} not found")
