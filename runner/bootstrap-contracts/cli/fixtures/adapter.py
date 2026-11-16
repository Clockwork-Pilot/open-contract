import json
import sys

request = json.load(sys.stdin)
results = []
scopes = []
for item in request["requests"]:
    results.append({"key": item["key"], "output": item["input"]})
    scopes.append({"key": item["key"], "files": [{"path": "validator.py", "lines": [1]}]})
print(json.dumps({
    "results": results,
    "coverage": {"sources": ["validator.py"]},
    "coverage_scopes": scopes,
    "coverage_lines": [{"path": "validator.py", "executable_lines": [1], "covered_lines": [1]}],
    "additional_test_coverage": {"executed": True, "passed": True, "native_report": {"format": "fixture"}},
    "additional_coverage_lines": [{"path": "validator.py", "executable_lines": [1], "covered_lines": [1]}]
}))
