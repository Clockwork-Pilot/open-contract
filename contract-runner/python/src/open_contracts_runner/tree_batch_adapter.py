"""Coordinate language-specific batches inside one established isolation."""

from __future__ import annotations

import json
import os
import subprocess
import sys


def main() -> int:
    try:
        request = json.load(sys.stdin)
        contracts = request["contracts"]
        javascript_adapter = os.environ.get("OPEN_CONTRACT_JS_ADAPTER")
        python_adapter = os.environ.get("OPEN_CONTRACT_PYTHON_ADAPTER")
        results = []
        for contract in contracts:
            validator = contract["validator"]
            suffix = validator.rpartition(".")[2].lower()
            if suffix == "py":
                if not python_adapter:
                    raise RuntimeError("Python adapter path was not provided by the runner runtime")
                command = [sys.executable, python_adapter]
            elif suffix in {"js", "mjs"}:
                if not javascript_adapter:
                    raise RuntimeError("JavaScript adapter path was not provided by the runner runtime")
                command = ["node", javascript_adapter]
            else:
                raise ValueError(f"unsupported validator extension: .{suffix}")
            completed = subprocess.run(
                command,
                input=json.dumps(contract, ensure_ascii=False),
                text=True,
                capture_output=True,
                check=False,
            )
            if completed.returncode:
                raise RuntimeError(
                    f"adapter for {contract['contract']} exited {completed.returncode}: "
                    f"{completed.stderr.strip()}"
                )
            result = json.loads(completed.stdout)
            results.append({"contract": contract["contract"], **result})
        print(json.dumps({"contracts": results}, ensure_ascii=False))
        return 0
    except Exception as exc:
        print(f"tree batch adapter: {type(exc).__name__}: {exc}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
