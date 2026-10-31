"""Single-case evaluator for the raw-dataset example."""

from typing import Any


def evaluate(entry: dict[str, Any]) -> dict[str, Any]:
    """Evaluate one normalized dataset entry and return its observation."""
    name = entry["input"]["name"]
    member = entry["input"]["member"]
    return {
        "greeting": f"Hello, {name}!",
        "recipient": {
            "name": name,
            "active": member["active"],
            "tier": member["tier"],
        },
    }
