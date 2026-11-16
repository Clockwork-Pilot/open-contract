"""A small Python contract validator exercised through the language adapter."""

from __future__ import annotations

def validate(value: dict[str, object]) -> dict[str, str]:
    name = value.get("name")
    if not isinstance(name, str):
        raise ValueError("name must be a string")
    return {"greeting": f"hello, {name}"}
