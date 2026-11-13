"""Map native check records into normalized runner dataset entries."""

from typing import Any


def read_dataset(source: dict[str, Any]) -> dict[str, Any]:
    """Rename the source fields while preserving their authored expectations."""
    cases = []
    for source_case in source["checks"]:
        case = {"id": source_case["case"], "input": source_case["request"]}
        if "result" in source_case:
            case["expected"] = source_case["result"]
        if "satisfies" in source_case:
            case["predicate"] = source_case["satisfies"]
        cases.append(case)

    return {"version": 1, "cases": cases}
