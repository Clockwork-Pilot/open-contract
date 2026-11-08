"""Optional Bubblewrap isolation for Open Contract dataset runs."""

from __future__ import annotations

import os
import secrets
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


class BubblewrapIsolationError(ValueError):
    """The requested Bubblewrap sandbox cannot be constructed or started."""


_SANDBOX_RUNNER_ROOT = Path("/open-contract")
_SANDBOX_OUTPUT_ROOT = Path("/outputs")
_SANDBOX_DEPENDENCY_ROOT = Path("/dependencies")


def run_with_bubblewrap(args: Any, child_arguments: list[str]) -> int:
    """Run a dataset with only the contract tree, runner, and declared inputs mounted.

    Contract sources and declared dependencies are mounted read-only. The only
    writable host mount is a temporary output directory used to return coverage
    data to the caller.
    """
    bwrap = shutil.which("bwrap")
    if bwrap is None:
        raise BubblewrapIsolationError("'bwrap' is not installed or not on PATH")
    if args.command != "run":
        raise BubblewrapIsolationError("Bubblewrap isolation is available for the 'run' command")
    if args.contract_root is None:
        raise BubblewrapIsolationError("--contract-root is required with --isolation bubblewrap")

    contract_root = args.contract_root.expanduser().resolve()
    if not contract_root.is_dir():
        raise BubblewrapIsolationError(f"contract root is not a directory: {contract_root}")
    dataset = args.dataset.expanduser().resolve()
    try:
        dataset_relative = dataset.relative_to(contract_root)
    except ValueError as exc:
        raise BubblewrapIsolationError("dataset must be inside --contract-root") from exc

    runner_root = _find_runner_root()
    dependency_roots = _resolve_dependency_roots(args.sandbox_dependency, contract_root, runner_root)
    sandbox_root = Path("/") / f"contract-run-{secrets.token_hex(12)}"
    pythonpath = _pythonpath(contract_root, runner_root, dependency_roots, sandbox_root)
    child_arguments = _rewrite_child_arguments(
        child_arguments, dataset_relative, contract_root, runner_root, dependency_roots, sandbox_root
    )

    coverage_path = args.coverage_file.expanduser().resolve()
    output_name = args.coverage_file.name
    with tempfile.TemporaryDirectory(prefix="open-contract-bwrap-output-") as temporary:
        temporary_output = Path(temporary) / output_name
        if args.coverage_append and coverage_path.is_file():
            shutil.copy2(coverage_path, temporary_output)
        command, environment = _build_command(
            bwrap=bwrap,
            contract_root=contract_root,
            runner_root=runner_root,
            dependency_roots=dependency_roots,
            pythonpath=pythonpath,
            sandbox_root=sandbox_root,
            child_arguments=child_arguments,
            output_directory=Path(temporary),
        )
        try:
            result = subprocess.run(command, env=environment, check=False)
        except OSError as exc:
            raise BubblewrapIsolationError(f"could not start bwrap: {exc}") from exc
        if (args.coverage or args.coverage_threshold is not None) and temporary_output.is_file():
            coverage_path.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(temporary_output, coverage_path)
        return result.returncode


def _find_runner_root() -> Path:
    """Locate the Open Contract checkout containing this runner installation."""
    cli_file = Path(__file__).resolve()
    for parent in cli_file.parents:
        if (parent / "contract-runner" / "python" / "src" / "open_contracts_runner").is_dir():
            return parent
    raise BubblewrapIsolationError("cannot locate the Open Contract runner checkout")


def _resolve_dependency_roots(
    requested: list[Path], contract_root: Path, runner_root: Path
) -> list[Path]:
    roots: list[Path] = []
    for value in requested:
        root = value.expanduser().resolve()
        if not root.is_dir():
            raise BubblewrapIsolationError(f"sandbox dependency is not a directory: {root}")
        if _is_within(root, contract_root) or _is_within(root, runner_root):
            continue
        if any(_is_within(root, previous) or _is_within(previous, root) for previous in roots):
            raise BubblewrapIsolationError("sandbox dependency roots must not overlap")
        roots.append(root)
    return roots


def _pythonpath(
    contract_root: Path,
    runner_root: Path,
    dependency_roots: list[Path],
    sandbox_root: Path,
) -> list[str]:
    paths = [str(sandbox_root), str(_SANDBOX_RUNNER_ROOT / "contract-runner/python/src")]
    predicate_src = runner_root / "bootstrap-contracts" / "predicates" / "python" / "src"
    if predicate_src.is_dir():
        paths.append(str(_SANDBOX_RUNNER_ROOT / "bootstrap-contracts/predicates/python/src"))

    prefixes = {Path(sys.prefix).resolve(), Path(sys.base_prefix).resolve()}
    for entry in os.environ.get("PYTHONPATH", "").split(os.pathsep):
        if not entry:
            continue
        host_path = Path(entry).expanduser()
        if not host_path.is_absolute():
            host_path = Path.cwd() / host_path
        host_path = host_path.resolve()
        mapped = _map_host_path(host_path, contract_root, runner_root, dependency_roots, sandbox_root)
        if mapped is not None:
            paths.append(mapped)
            continue
        if any(_is_within(host_path, prefix) for prefix in prefixes):
            continue
        if host_path.exists():
            raise BubblewrapIsolationError(
                f"PYTHONPATH entry is outside mounted roots: {host_path}; "
                "declare it with --sandbox-dependency"
            )
    return list(dict.fromkeys(paths))


def _map_host_path(
    path: Path,
    contract_root: Path,
    runner_root: Path,
    dependency_roots: list[Path],
    sandbox_root: Path,
) -> str | None:
    if _is_within(path, runner_root):
        return str(_SANDBOX_RUNNER_ROOT / path.relative_to(runner_root))
    if _is_within(path, contract_root):
        return str(sandbox_root / "contracts" / path.relative_to(contract_root))
    for index, root in enumerate(dependency_roots):
        if _is_within(path, root):
            return str(_SANDBOX_DEPENDENCY_ROOT / str(index) / path.relative_to(root))
    # A repository may keep executable contracts under a top-level
    # ``contracts/`` package. Map the repository import root onto the random
    # sandbox root; only its mounted ``contracts/`` child is present there.
    if path == contract_root.parent or (contract_root.name == "contracts" and path == contract_root.parent.parent):
        return str(sandbox_root)
    return None


def _rewrite_child_arguments(
    child_arguments: list[str],
    dataset_relative: Path,
    contract_root: Path,
    runner_root: Path,
    dependency_roots: list[Path],
    sandbox_root: Path,
) -> list[str]:
    rewritten = list(child_arguments)
    contract_mount = sandbox_root / "contracts"
    rewritten[1] = str(contract_mount / dataset_relative)
    index = 0
    while index < len(rewritten):
        value = rewritten[index]
        if value == "--coverage-file":
            rewritten[index + 1] = str(_SANDBOX_OUTPUT_ROOT / Path(rewritten[index + 1]).name)
            index += 2
            continue
        if value == "--coverage-source" and index + 1 < len(rewritten):
            source = rewritten[index + 1]
            source_path = Path(source).expanduser()
            if source_path.is_absolute():
                mapped = _map_host_path(
                    source_path.resolve(), contract_root, runner_root, dependency_roots, sandbox_root
                )
                if mapped is None:
                    raise BubblewrapIsolationError(
                        f"coverage source is outside mounted roots: {source_path}"
                    )
                rewritten[index + 1] = mapped
            index += 2
            continue
        index += 1
    return rewritten


def _build_command(
    *,
    bwrap: str,
    contract_root: Path,
    runner_root: Path,
    dependency_roots: list[Path],
    pythonpath: list[str],
    sandbox_root: Path,
    child_arguments: list[str],
    output_directory: Path,
) -> tuple[list[str], dict[str, str]]:
    command = [bwrap, "--die-with-parent", "--new-session", "--unshare-all", "--clearenv"]

    # Expose only the standard runtime, the active Python environment, the
    # Open Contract checkout, the executable contract tree, and declared deps.
    for path in ("/usr", "/bin", "/sbin", "/lib", "/lib64", "/etc"):
        host_path = Path(path)
        if host_path.exists():
            command.extend(("--dir", path, "--ro-bind-try", path, path))
    command.extend(("--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp"))

    for runtime in dict.fromkeys((Path(sys.prefix).resolve(), Path(sys.base_prefix).resolve())):
        if not runtime.exists() or any(_is_within(runtime, Path(root)) for root in ("/usr", "/bin", "/lib", "/lib64")):
            continue
        _ensure_parent_directories(command, runtime)
        command.extend(("--dir", str(runtime)))
        command.extend(("--ro-bind-try", str(runtime), str(runtime)))

    contract_mount = sandbox_root / "contracts"
    work_mount = sandbox_root / "work"
    command.extend(("--dir", str(sandbox_root)))
    command.extend(("--dir", str(contract_mount), "--ro-bind", str(contract_root), str(contract_mount)))
    command.extend(("--dir", str(_SANDBOX_RUNNER_ROOT)))
    command.extend(("--ro-bind", str(runner_root), str(_SANDBOX_RUNNER_ROOT)))

    for index, root in enumerate(dependency_roots):
        destination = _SANDBOX_DEPENDENCY_ROOT / str(index)
        _ensure_parent_directories(command, destination)
        command.extend(("--dir", str(destination)))
        command.extend(("--ro-bind", str(root), str(destination)))

    command.extend(("--dir", str(_SANDBOX_OUTPUT_ROOT)))
    command.extend(("--bind", str(output_directory), str(_SANDBOX_OUTPUT_ROOT)))
    home_mount = sandbox_root / "home"
    command.extend(("--dir", str(home_mount)))
    command.extend(("--dir", str(work_mount)))
    command.extend(("--setenv", "HOME", str(home_mount)))
    command.extend(("--setenv", "PATH", f"{sys.prefix}/bin:/usr/local/bin:/usr/bin:/bin"))
    command.extend(("--setenv", "PYTHONPATH", os.pathsep.join(pythonpath)))
    command.extend(("--setenv", "PYTHONDONTWRITEBYTECODE", "1"))
    command.extend(("--setenv", "PYTHONNOUSERSITE", "1"))

    command.extend(("--chdir", str(work_mount), "--", sys.executable, "-m", "open_contracts_runner", *child_arguments))
    return command, {}


def _ensure_parent_directories(command: list[str], path: Path) -> None:
    for parent in reversed(path.parents):
        if str(parent) == "/":
            continue
        command.extend(("--dir", str(parent)))


def _is_within(path: Path, parent: Path) -> bool:
    try:
        path.relative_to(parent)
    except ValueError:
        return False
    return True
