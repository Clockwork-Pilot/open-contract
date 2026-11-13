"""Execute one contract validator without importing verification code."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any


def run_validator(validator: Path, root: Path, value: Any) -> Any:
    suffix = validator.suffix.lower()
    if suffix == ".py":
        code = (
            "import importlib.util,json,sys; "
            "root,source=sys.argv[1:3]; sys.path.insert(0,root); "
            "name='open_contract_validator'; spec=importlib.util.spec_from_file_location(name,source); "
            "module=importlib.util.module_from_spec(spec); sys.modules[name]=module; spec.loader.exec_module(module); "
            "fn=getattr(module,'validate'); print(json.dumps(fn(json.load(sys.stdin))))"
        )
        command = [sys.executable, "-c", code, str(root), str(validator)]
    elif suffix in {".js", ".mjs"}:
        code = (
            "const m=await import((await import('node:url')).pathToFileURL(process.argv[1])); "
            "const f=m.validate??m.default; if(typeof f!=='function') throw Error('validator must export validate(input) or default'); "
            "let s=''; for await(const c of process.stdin)s+=c; console.log(JSON.stringify(await f(JSON.parse(s))));"
        )
        node = shutil.which("node")
        if not node:
            raise RuntimeError("Node.js is required to run a JavaScript contract validator")
        command = [node, "--input-type=module", "-e", code, str(validator)]
    else:
        raise ValueError(f"unsupported validator file type: {suffix}")
    completed = subprocess.run(command, input=json.dumps(value, ensure_ascii=False), text=True,
                               capture_output=True, cwd=root, timeout=60, check=False)
    if completed.returncode:
        raise RuntimeError(f"validator exited {completed.returncode}: {completed.stderr.strip()}")
    try:
        return json.loads(completed.stdout)
    except json.JSONDecodeError as exc:
        raise ValueError("validator must print one JSON value to stdout") from exc


def serve(root_path: str) -> int:
    root = Path(root_path).resolve(strict=True)
    for line in sys.stdin:
        try:
            request = json.loads(line)
            validator = (root / request["validator"]).resolve(strict=True)
            if not validator.is_relative_to(root):
                raise ValueError("validator escaped package root")
            output = run_validator(validator, root, request["input"])
            response = {"output": output}
        except Exception as exc:
            response = {"error": f"{type(exc).__name__}: {exc}"}
        print(json.dumps(response), flush=True)
    return 0
