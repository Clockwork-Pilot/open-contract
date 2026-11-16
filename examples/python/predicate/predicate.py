"""Standalone evaluator for bootstrap predicate dataset v1."""

from __future__ import annotations

import ast
import json
import re
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

_MISSING = object()
_INVALID = object()
_TOKEN = re.compile(
    r"\s*(?:(?P<op>==|!=|>=|<=|>|<)|(?P<paren>[()])|"
    r"(?P<string>'(?:\\.|[^'\\])*'|\"(?:\\.|[^\"\\])*\")|"
    r"(?P<number>-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)|"
    r'(?P<field>(?:\.[A-Za-z0-9_@-]+|\["(?:\\.|[^"\\])*"\])+)|'
    r"(?P<word>[A-Za-z_][A-Za-z0-9_]*))"
)
_DIGEST = re.compile(r"^(?:sha256:)?([0-9a-fA-F]{64})$")


@dataclass(frozen=True)
class _Field:
    path: str


def evaluate_predicate(predicate: str, record: Any) -> bool:
    """Evaluate a string predicate expression against a JSON value."""
    return _ExpressionParser(predicate, record).evaluate()


class _ExpressionParser:
    def __init__(self, expression: str, record: Any) -> None:
        expression = expression.strip()
        self.record = record
        self.tokens: list[tuple[str, Any]] = []
        self.index = 0
        position = 0
        while position < len(expression):
            match = _TOKEN.match(expression, position)
            if not match:
                self.tokens = []
                self.index = 0
                return
            position = match.end()
            kind = match.lastgroup
            assert kind is not None
            value: Any = match.group(kind)
            if kind == "string":
                try:
                    value = ast.literal_eval(value)
                except (SyntaxError, ValueError):
                    self.tokens = []
                    return
            elif kind == "number":
                value = float(value) if any(c in value for c in ".eE") else int(value)
            self.tokens.append((kind, value))

    def evaluate(self) -> bool:
        if not self.tokens:
            return False
        try:
            result = self._parse_or()
            return self.index == len(self.tokens) and result is True
        except (IndexError, TypeError, ValueError, OverflowError):
            return False

    def _peek(self) -> tuple[str, Any] | None:
        return self.tokens[self.index] if self.index < len(self.tokens) else None

    def _take(self) -> tuple[str, Any]:
        token = self._peek()
        if token is None:
            raise ValueError("unexpected end of expression")
        self.index += 1
        return token

    def _word(self, expected: str) -> bool:
        token = self._peek()
        if token and token[0] == "word" and token[1].upper() == expected:
            self.index += 1
            return True
        return False

    def _parse_or(self) -> bool:
        result = self._parse_and()
        while self._word("OR"):
            right = self._parse_and()
            result = right or result
        return result

    def _parse_and(self) -> bool:
        result = self._parse_not()
        while self._word("AND"):
            right = self._parse_not()
            result = result and right
        return result

    def _parse_not(self) -> bool:
        if self._word("NOT"):
            return not self._parse_not()
        return self._parse_comparison()

    def _parse_comparison(self) -> bool:
        left = self._parse_primary()
        token = self._peek()
        if token and token[0] == "op":
            operator = self._take()[1]
            right = self._parse_primary()
            return _compare(_resolve(left, self.record), _resolve(right, self.record), operator)
        return left is True

    def _parse_primary(self) -> Any:
        token = self._take()
        kind, value = token
        if kind == "paren" and value == "(":
            result = self._parse_or()
            closing = self._take()
            if closing != ("paren", ")"):
                raise ValueError("unclosed parenthesis")
            return result
        if kind == "field":
            return _Field(value)
        if kind == "string" or kind == "number":
            return value
        if kind == "word":
            upper = value.upper()
            if upper == "TRUE":
                return True
            if upper == "FALSE":
                return False
            if upper == "NULL":
                return None
            if upper == "EXISTS":
                if self._take() != ("paren", "("):
                    raise ValueError("exists requires parentheses")
                argument = self._take()
                if argument[0] == "field":
                    target: Any = _Field(argument[1])
                else:
                    raise ValueError("exists requires a field path")
                if self._take() != ("paren", ")"):
                    raise ValueError("exists takes one argument")
                value = _resolve(target, self.record)
                return _has_value(value)
        return _INVALID


def _resolve(value: Any, record: Any) -> Any:
    if isinstance(value, _Field):
        return _lookup(record, value.path)
    return value


def _lookup(record: Any, path: str) -> Any:
    current = record
    parts = [
        match.group(1) if match.group(1) is not None else json.loads(match.group(2))
        for match in re.finditer(r'\.([A-Za-z0-9_@-]+)|\[("(?:\\.|[^"\\])*")\]', path)
    ]
    for part in parts:
        if isinstance(current, Mapping) and part in current:
            current = current[part]
        elif isinstance(current, list) and re.fullmatch(r"0|[1-9][0-9]*", part):
            index = int(part)
            if index >= len(current):
                return _MISSING
            current = current[index]
        else:
            return _MISSING
    return current


def _has_value(value: Any) -> bool:
    return value is not _MISSING and value is not None and value != {} and value != []


def _strict_equal(left: Any, right: Any) -> bool:
    if left is _MISSING:
        left = None
    if right is _MISSING:
        right = None
    if isinstance(left, bool) != isinstance(right, bool):
        return False
    if isinstance(left, (int, float)) and not isinstance(left, bool) and isinstance(right, (int, float)) and not isinstance(right, bool):
        return left == right
    if isinstance(left, str) and isinstance(right, str):
        left_digest = _DIGEST.fullmatch(left)
        right_digest = _DIGEST.fullmatch(right)
        if left_digest and right_digest:
            return left_digest.group(1).casefold() == right_digest.group(1).casefold()
    return type(left) is type(right) and left == right


def _order_key(value: Any) -> tuple[str, Any] | None:
    if isinstance(value, bool):
        return "bool", value
    if isinstance(value, (int, float)):
        return "number", value
    if isinstance(value, str):
        digest = _DIGEST.fullmatch(value)
        return "string", digest.group(1).casefold() if digest else value
    return None


def _compare(left: Any, right: Any, operator: str) -> bool:
    if left is _INVALID or right is _INVALID:
        return False
    if operator == "==":
        return _strict_equal(left, right)
    if operator == "!=":
        return not _strict_equal(left, right)
    left_key = _order_key(None if left is _MISSING else left)
    right_key = _order_key(None if right is _MISSING else right)
    if left_key is None or right_key is None or left_key[0] != right_key[0]:
        return False
    return {
        ">": left_key[1] > right_key[1],
        ">=": left_key[1] >= right_key[1],
        "<": left_key[1] < right_key[1],
        "<=": left_key[1] <= right_key[1],
    }[operator]
