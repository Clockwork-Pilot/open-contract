from pathlib import PurePosixPath

import yaml


def _relative(path):
    return (
        isinstance(path, str)
        and bool(path)
        and "\\" not in path
        and not PurePosixPath(path).is_absolute()
        and not any(part in {"", ".", ".."} for part in path.split("/"))
        and ":" not in path.split("/", 1)[0]
    )


def validate(value):
    operation = value["operation"]
    if operation == "path":
        return {"valid": _relative(value["path"])}
    if operation == "missing-manifest":
        return {"valid": False}
    if operation != "manifest":
        raise ValueError(f"unknown operation: {operation}")

    try:
        manifest = yaml.safe_load(value["yaml"])
    except yaml.YAMLError:
        return {"valid": False}
    if not isinstance(manifest, dict) or manifest.get("version") != "1":
        return {"valid": False}
    if not isinstance(manifest.get("name"), str) or not manifest["name"].strip():
        return {"valid": False}
    files = manifest.get("files", {})
    filesystem = manifest.get("filesystem", {})
    read = filesystem.get("read", [])
    write = filesystem.get("write", [])
    declared = [files.get("dataset"), files.get("validator")]
    declared.extend(files.get("support", []))
    declared.extend(files.get("nested_contracts", []))
    paths = [*declared, *read, *write]
    if any(not _relative(path) for path in paths):
        return {"valid": False}
    if not all(path in read or any(path.startswith(root.rstrip("/") + "/") for root in read) for path in declared):
        return {"valid": False}
    if value.get("symlink_escape"):
        return {"valid": False}
    if any(path == "missing.json" for path in paths):
        return {"valid": False}
    if files.get("dataset") == "directory":
        return {"valid": False}
    return {"valid": True}
