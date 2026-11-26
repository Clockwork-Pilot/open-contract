import json
import os
import subprocess
import sys
from pathlib import Path


LANGUAGE_PACKAGE = Path(__file__).parents[1] / "src"


def test_coverage_lines_are_normalized_and_subset_of_executable_lines(tmp_path):
    (tmp_path / "helper.py").write_text(
        "def answer(value):\n"
        "    return {'ok': value}\n",
        encoding="utf-8",
    )
    (tmp_path / "validator.py").write_text(
        "from helper import answer\n"
        "def validate(value, context):\n"
        "    if value:\n"
        "        return answer(value)\n"
        "    return {'ok': False}\n",
        encoding="utf-8",
    )
    entrypoint = tmp_path / "contract_entry.py"
    entrypoint.write_text(
        "import json, sys\n"
        f"sys.path.insert(0, {str(LANGUAGE_PACKAGE)!r})\n"
        "from open_contract_language.batch_adapter import run_dataset\n"
        "def load_validator():\n"
        "    from validator import validate\n"
        "    return validate\n"
        "print(json.dumps(run_dataset(json.load(sys.stdin), validator_loader=load_validator)))\n",
        encoding="utf-8",
    )
    request = {
        "package": ".",
        "coverage_sources": ["validator.py"],
        "requests": [
            {"key": {"case_id": "yes", "step_id": None}, "input": True},
            {"key": {"case_id": "no", "step_id": None}, "input": False},
        ],
    }
    result = subprocess.run(
        [sys.executable, str(entrypoint)],
        input=json.dumps(request),
        text=True,
        capture_output=True,
        cwd=tmp_path,
        env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
        check=False,
    )

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert len(report["coverage_lines"]) == 1
    assert report["coverage"]["sources"] == ["validator.py"]
    assert list(report["coverage"]["files"]) == ["validator.py"]
    source = report["coverage_lines"][0]
    assert source["path"] == "validator.py"
    assert source["executable_lines"] == sorted(set(source["executable_lines"]))
    assert source["covered_lines"] == sorted(set(source["covered_lines"]))
    assert set(source["covered_lines"]) <= set(source["executable_lines"])
    assert source["covered_lines"]
    assert report["coverage_metrics"]["line"]["covered"] == len(source["covered_lines"])
    assert report["coverage_metrics"]["statement"]["total"] > 0
    assert report["coverage_metrics"]["statement"]["covered"] == report["coverage_metrics"]["statement"]["total"]
    assert report["coverage_metrics"]["branch"]["total"] > 0
    assert report["coverage_metrics"]["branch"]["covered"] == report["coverage_metrics"]["branch"]["total"]
    assert "function" not in report["coverage_metrics"]
