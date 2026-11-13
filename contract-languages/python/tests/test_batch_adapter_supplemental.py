import json
import os
import subprocess
import sys
from pathlib import Path


ADAPTER = Path(__file__).parents[1] / "src/open_contracts_runner/batch_adapter.py"


def run_adapter(tmp_path, supplemental_tests, validator_source=None):
    (tmp_path / "validator.py").write_text(
        validator_source or "def validate(value):\n    return {\"value\": value}\n", encoding="utf-8"
    )
    request = {
        "validator": "validator.py",
        "package": ".",
        "coverage_sources": ["validator.py"],
        "supplemental_tests": supplemental_tests,
        "requests": [{"key": {"case": "one"}, "input": 3}],
    }
    return subprocess.run(
        [sys.executable, str(ADAPTER)],
        input=json.dumps(request),
        text=True,
        capture_output=True,
        cwd=tmp_path,
        env={**os.environ, "PYTHONDONTWRITEBYTECODE": "1"},
        check=False,
    )


def test_supplemental_tests_report_separately_from_dataset_coverage(tmp_path):
    (tmp_path / "supplemental.py").write_text(
        "from validator import validate\n\ndef run():\n    assert validate(4) == {\"value\": 4}\n",
        encoding="utf-8",
    )

    result = run_adapter(tmp_path, ["supplemental.py"])

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["additional_test_coverage"]["status"] == "passed"
    assert report["additional_test_coverage"]["executed"] is True
    assert report["coverage_lines"] == report["additional_coverage_lines"]
    assert report["coverage_lines"][0]["path"] == "validator.py"


def test_supplemental_test_failure_is_reported_without_changing_dataset_result(tmp_path):
    (tmp_path / "supplemental.py").write_text(
        "def run():\n    raise ValueError('deliberate failure')\n", encoding="utf-8"
    )

    result = run_adapter(tmp_path, ["supplemental.py"])

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["results"] == [{"key": {"case": "one"}, "output": {"value": 3}}]
    assert report["additional_test_coverage"]["status"] == "failed"
    assert "deliberate failure" in report["additional_test_coverage"]["error"]


def test_supplemental_tests_are_skipped_below_dataset_coverage_threshold(tmp_path):
    marker = tmp_path / "was-run"
    (tmp_path / "supplemental.py").write_text(
        f"from pathlib import Path\ndef run():\n    Path({str(marker)!r}).write_text('yes')\n",
        encoding="utf-8",
    )

    result = run_adapter(
        tmp_path,
        ["supplemental.py"],
        "def validate(value):\n    if value:\n        return {'value': value}\n    return {'empty': True}\n",
    )

    assert result.returncode == 0, result.stderr
    report = json.loads(result.stdout)
    assert report["additional_test_coverage"]["status"] == "skipped"
    assert report["additional_test_coverage"]["executed"] is False
    assert not marker.exists()
