"""Purely adapt decoded profile data into normalized runner dataset entries."""

import json
from typing import Any


def read_dataset(source: dict[str, Any]) -> dict[str, Any]:
    """Transform source profiles into exact and predicate cases."""
    profiles = source.get("profiles")
    if not isinstance(profiles, list) or not all(
        isinstance(profile, dict)
        and isinstance(profile.get("display_name"), str)
        and isinstance(profile.get("active"), bool)
        and isinstance(profile.get("tier"), str)
        for profile in profiles
    ):
        raise ValueError(
            "profiles must contain display_name, active, and tier values"
        )

    cases = []
    for index, profile in enumerate(profiles):
        name = profile["display_name"]
        case_input = {
            "name": name,
            "member": {"active": profile["active"], "tier": profile["tier"]},
        }
        expected = {
            "greeting": f"Hello, {name}!",
            "recipient": {
                "name": name,
                "active": profile["active"],
                "tier": profile["tier"],
            },
        }
        predicate = (
            f"exists(.recipient.name) AND "
            f".recipient.name == {json.dumps(name)} AND "
            "(.recipient.active == true OR "
            '.recipient.tier == "standard") AND '
            'NOT (.recipient.tier == "blocked") AND '
            "exists(.recipient.tier) AND "
            f".greeting == {json.dumps(expected['greeting'])}"
        )

        cases.extend(
            [
                {
                    "id": f"ingested-profile-{index + 1}-exact",
                    "input": case_input,
                    "expected": expected,
                },
                {
                    "id": f"ingested-profile-{index + 1}-compound-predicate",
                    "input": case_input,
                    "predicate": predicate,
                },
            ]
        )

    return {"version": 1, "cases": cases}
