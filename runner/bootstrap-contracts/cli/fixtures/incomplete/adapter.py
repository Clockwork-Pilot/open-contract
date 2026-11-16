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
    "coverage": {"sources": []},
    "coverage_scopes": scopes,
    "coverage_lines": [{"path": "validator.py", "executable_lines": [1], "covered_lines": [1]}],
}))
