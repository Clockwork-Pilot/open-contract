import json
from pathlib import Path
import subprocess
import sys


def validate(value):
    operation = value["operation"]
    if operation == "stage":
        dataset = json.loads(Path("dataset.json").read_text(encoding="utf-8"))
        return {
            "dataset_staged": bool(dataset["cases"]),
            "validator_staged": Path("validator.py").is_file(),
            "support_staged": Path("support/helper.py").is_file(),
            "nested_dataset_staged": Path("nested/dataset.json").is_file(),
            "fresh_root": True,
        }
    if operation == "command":
        result = subprocess.run(
            [sys.executable, "-c", "import sys; sys.stdout.write(sys.stdin.read())"],
            input=value["value"],
            capture_output=True,
            text=True,
            check=True,
        )
        return {"stdout": result.stdout, "success": result.returncode == 0}
    if operation == "empty-command":
        try:
            subprocess.run([], check=True)
        except (IndexError, ValueError):
            return {"rejected": True}
        return {"rejected": False}
    raise ValueError(f"unknown isolation dataset operation: {operation}")
