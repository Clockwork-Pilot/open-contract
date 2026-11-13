"""Read and stage a versioned Open Contract package manifest."""

from __future__ import annotations

import shutil
import tempfile
from contextlib import contextmanager
from pathlib import Path, PurePosixPath
from typing import Any

import yaml


def read_manifest(path: str | Path) -> tuple[dict[str, Any], Path]:
    manifest_path = Path(path).resolve(strict=True)
    value = yaml.safe_load(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(value, dict) or set(value) != {"version", "name", "files", "filesystem"}:
        raise ValueError("manifest must contain exactly version, name, files, and filesystem")
    if value["version"] != "1" or not isinstance(value["name"], str) or not value["name"]:
        raise ValueError("manifest version must be '1' and name must be a non-empty string")
    files = value["files"]
    policy = value["filesystem"]
    if not isinstance(files, dict) or set(files) - {"dataset", "validator", "support", "nested_contracts"}:
        raise ValueError("manifest files must contain dataset, validator, optional support, and nested_contracts")
    if not isinstance(files.get("dataset"), str) or not isinstance(files.get("validator"), str):
        raise ValueError("manifest files.dataset and files.validator must be package-relative paths")
    if not isinstance(policy, dict) or set(policy) != {"read", "write"}:
        raise ValueError("manifest filesystem must contain exactly read and write")
    if any(not isinstance(policy.get(key), list) for key in ("read", "write")):
        raise ValueError("manifest filesystem.read and filesystem.write must be arrays")
    if any(not isinstance(files.get(key, []), list) for key in ("support", "nested_contracts")):
        raise ValueError("manifest support and nested_contracts must be arrays")

    package_root = manifest_path.parent
    paths: list[str] = [files["dataset"], files["validator"]]
    paths.extend(files.get("support", []))
    paths.extend(files.get("nested_contracts", []))
    paths.extend(policy["read"])
    paths.extend(policy["write"])
    for name in paths:
        _validate_relative_path(name)

    for name in paths[: 2 + len(files.get("support", [])) + len(files.get("nested_contracts", []))]:
        resolved = (package_root / PurePosixPath(name)).resolve(strict=True)
        if not resolved.is_relative_to(package_root):
            raise ValueError(f"manifest path escapes its package through a symlink: {name}")
        if not resolved.is_file():
            raise ValueError(f"manifest file is not a regular file: {name}")
    for name in policy["read"]:
        resolved = (package_root / PurePosixPath(name)).resolve(strict=True)
        if not resolved.is_relative_to(package_root):
            raise ValueError(f"manifest read path escapes its package through a symlink: {name}")
    for name in files.get("support", []):
        if not any(name == allowed or name.startswith(f"{allowed}/") for allowed in policy["read"]):
            raise ValueError(f"support file is not readable under filesystem.read: {name}")
    for name in (files["dataset"], files["validator"], *files.get("nested_contracts", [])):
        if not any(name == allowed or name.startswith(f"{allowed}/") for allowed in policy["read"]):
            raise ValueError(f"contract file is not readable under filesystem.read: {name}")
    for name in policy["write"]:
        candidate = package_root / PurePosixPath(name)
        if candidate.exists():
            resolved = candidate.resolve(strict=True)
            if not resolved.is_relative_to(package_root):
                raise ValueError(f"manifest write path escapes its package through a symlink: {name}")
    return value, package_root


def stage_package(manifest: dict[str, Any], source_root: Path) -> tempfile.TemporaryDirectory[str]:
    """Copy only manifest-listed files into a fresh randomized package root."""
    stage = tempfile.TemporaryDirectory(prefix="open-contract-")
    root = Path(stage.name)
    names = [manifest["files"]["dataset"], manifest["files"]["validator"]]
    names.extend(manifest["files"].get("support", []))
    names.extend(manifest["files"].get("nested_contracts", []))
    for name in names:
        source = (source_root / PurePosixPath(name)).resolve(strict=True)
        if not source.is_relative_to(source_root):
            stage.cleanup()
            raise ValueError(f"manifest path escapes its package through a symlink: {name}")
        destination = root / PurePosixPath(name)
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, destination)
    for name in manifest["filesystem"]["write"]:
        destination = root / PurePosixPath(name)
        if destination.exists():
            continue
        # Treat a declared suffix as a file, otherwise as a writable directory.
        if PurePosixPath(name).suffix:
            destination.parent.mkdir(parents=True, exist_ok=True)
            destination.touch()
        else:
            destination.mkdir(parents=True, exist_ok=True)
    return stage


@contextmanager
def stage_contract_tree(manifest: dict[str, Any], source_root: Path, manifest_path: Path):
    """Stage every declared file in a nested tree and return root-relative policies."""
    stage = tempfile.TemporaryDirectory(prefix="open-contract-tree-")
    target_root = Path(stage.name)
    read_paths: set[str] = set()
    write_paths: set[str] = set()
    source_map: dict[str, str] = {}

    def copy_tree(current: dict[str, Any], current_source: Path, prefix: PurePosixPath, current_manifest: Path, stack: frozenset[Path]) -> None:
        current_manifest = current_manifest.resolve(strict=True)
        if current_manifest in stack:
            raise ValueError(f"nested contract cycle detected at {current_manifest}")
        stack = stack | {current_manifest}
        current_files = current["files"]
        for name in [current_files["dataset"], current_files["validator"], *current_files.get("support", [])]:
            source = (current_source / name).resolve(strict=True)
            if not source.is_relative_to(current_source.resolve()):
                raise ValueError(f"manifest path escapes its package through a symlink: {name}")
            destination_name = prefix / PurePosixPath(name)
            destination = target_root / destination_name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(source, destination)
            source_map[str(destination)] = str(source)
        for name in current["filesystem"]["read"]:
            read_paths.add((prefix / PurePosixPath(name)).as_posix())
        for name in current["filesystem"]["write"]:
            mapped = prefix / PurePosixPath(name)
            write_paths.add(mapped.as_posix())
            destination = target_root / mapped
            if not destination.exists():
                if mapped.suffix:
                    destination.parent.mkdir(parents=True, exist_ok=True)
                    destination.touch()
                else:
                    destination.mkdir(parents=True, exist_ok=True)
        for child in current_files.get("nested_contracts", []):
            child_path = (current_source / child).resolve(strict=True)
            if not child_path.is_relative_to(current_source.resolve()):
                raise ValueError(f"nested manifest escapes its package: {child}")
            child_manifest, child_root = read_manifest(child_path)
            child_rel = PurePosixPath(child)
            read_paths.add((prefix / child_rel).as_posix())
            child_dest = target_root / prefix / child_rel
            child_dest.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(child_path, child_dest)
            source_map[str(child_dest)] = str(child_path)
            child_prefix = prefix / child_rel.parent
            copy_tree(child_manifest, child_root, child_prefix, child_path, stack)

    try:
        copy_tree(manifest, source_root, PurePosixPath(""), manifest_path, frozenset())
        yield target_root, sorted(read_paths), sorted(write_paths), source_map
    finally:
        stage.cleanup()


def _validate_relative_path(value: Any) -> None:
    if not isinstance(value, str) or not value or "\\" in value:
        raise ValueError(f"manifest path must be a non-empty POSIX package path: {value!r}")
    path = PurePosixPath(value)
    if path.is_absolute() or any(part in {"", ".", ".."} for part in path.parts) or ":" in path.parts[0]:
        raise ValueError(f"manifest path must stay within the package: {value}")
