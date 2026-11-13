"""Contract-owned entrypoint for the Python predicate example."""

from __future__ import annotations

import json
import os
import sys

def load_validator():
    # Imported after coverage starts so validator initialization is measured.
    from validator import validate

    return validate


def main() -> int:
    try:
        request = json.load(sys.stdin)
        language_path = os.environ.get("OPEN_CONTRACT_LANGUAGE_PATH")
        if not language_path:
            raise RuntimeError("OPEN_CONTRACT_LANGUAGE_PATH is not configured")
        sys.path.insert(0, language_path)
        from open_contract_language.batch_adapter import run_dataset

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
