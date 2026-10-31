"""Single-case evaluator for the ingested-dataset example."""

from typing import Any


def evaluate(input_data: dict[str, Any]) -> dict[str, Any]:
    """Return the observed greeting for one case input."""
    name = input_data["name"]
    member = input_data["member"]
    return {
        "greeting": f"Hello, {name}!",
        "recipient": {
            "name": name,
            "active": member["active"],
            "tier": member["tier"],
        },
    }
