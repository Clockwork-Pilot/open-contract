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
    coverage.start()
    try:
        with contextlib.redirect_stdout(sys.stderr):
            try:
                executor = _load_validator(validator, root)
            except Exception as exc:
                for item in request.get("requests", []):
                    results.append({"key": item["key"], "error": f"{type(exc).__name__}: {exc}"})
            else:
                for item in request.get("requests", []):
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
    for name in sorted(data.measured_files()):
        source = Path(name).resolve()
        if not source.is_relative_to(root):
            continue
        relative = source.relative_to(root).as_posix()
        files[relative] = {"lines": data.lines(name) or [], "arcs": data.arcs(name) or []}
    native = {"format": "coverage.py", "sources": sorted(files), "files": files}
    print(json.dumps({"results": results, "coverage": native}, ensure_ascii=False))
    return 0


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
