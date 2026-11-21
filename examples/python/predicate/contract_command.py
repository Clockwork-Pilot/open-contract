"""Contract-owned entrypoint for the Python predicate contract example."""

from __future__ import annotations

import json
import importlib.util
import os
import sys
from pathlib import Path

def load_validator():
    # Imported after coverage starts so validator initialization is measured.
    from validator import validate

    return validate


def main() -> int:
    try:
        request = json.load(sys.stdin)
        adapter_entrypoint = os.environ.get("OPEN_CONTRACT_LANGUAGE_ENTRYPOINT")
        if not adapter_entrypoint:
            raise RuntimeError("OPEN_CONTRACT_LANGUAGE_ENTRYPOINT is required")
        spec = importlib.util.spec_from_file_location(
            "open_contract_language.batch_adapter", Path(adapter_entrypoint)
        )
        if spec is None or spec.loader is None:
            raise RuntimeError("cannot load language adapter entrypoint")
        module = importlib.util.module_from_spec(spec)
        sys.modules[spec.name] = module
        spec.loader.exec_module(module)
        run_dataset = module.run_dataset

        report = run_dataset(request, validator_loader=load_validator)
        print(json.dumps(report, ensure_ascii=False))
        return 2 if report.get("error") else 0
    except Exception as error:
        print(json.dumps({
            "results": [],
            "coverage": None,
            "error": f"{type(error).__name__}: {error}",
        }))
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
