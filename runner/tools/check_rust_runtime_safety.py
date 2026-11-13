#!/usr/bin/env python3
"""Reject panic-prone Rust constructs outside test-only modules."""

from __future__ import annotations

import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
RUNTIME_ROOTS = [ROOT / "runner" / "rust" / "src", ROOT / "runner" / "bootstrap-contracts"]
TEST_MODULE = re.compile(
    r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]\s*"
    r"(?:(?:pub(?:\([^)]*\))?)\s+)?mod\s+[A-Za-z_][A-Za-z_0-9]*\s*\{"
)
FORBIDDEN = [
    (re.compile(r"\b(?:assert|debug_assert|panic|todo|unimplemented|unreachable)\s*!"), "panic-prone macro"),
    (re.compile(r"\.\s*(?:unwrap|expect)\s*\("), "panic-prone method"),
]


def mask_non_code(source: str) -> str:
    """Replace comments and literals with spaces while retaining newlines."""
    result = list(source)
    index = 0
    length = len(source)
    while index < length:
        raw = _raw_string_end(source, index)
        if raw is not None:
            end, terminator = raw
            close = source.find(terminator, end)
            close = length if close < 0 else close + len(terminator)
            _blank(result, index, close)
            index = close
            continue
        if source.startswith("//", index):
            close = source.find("\n", index)
            close = length if close < 0 else close
            _blank(result, index, close)
            index = close
            continue
        if source.startswith("/*", index):
            close = _block_comment_end(source, index)
            _blank(result, index, close)
            index = close
            continue
        if source[index] == '"':
            close = _quoted_end(source, index, '"')
            _blank(result, index, close)
            index = close
            continue
        if source[index] == "'":
            close = _char_literal_end(source, index)
            if close is not None:
                _blank(result, index, close)
                index = close
                continue
        index += 1
    return "".join(result)


def _raw_string_end(source: str, index: int) -> tuple[int, str] | None:
    start = index
    if source.startswith("br", index):
        index += 2
    elif source.startswith("r", index):
        index += 1
    else:
        return None
    while index < len(source) and source[index] == "#":
        index += 1
    if index >= len(source) or source[index] != '"':
        return None
    hashes = source[start:index].count("#")
    return index + 1, '"' + ("#" * hashes)


def _quoted_end(source: str, start: int, quote: str) -> int:
    index = start + 1
    while index < len(source):
        if source[index] == "\\":
            index += 2
        elif source[index] == quote:
            return index + 1
        else:
            index += 1
    return len(source)


def _char_literal_end(source: str, start: int) -> int | None:
    close = _quoted_end(source, start, "'")
    if close == len(source) and (not source or source[-1] != "'"):
        return None
    if "\n" in source[start:close]:
        return None
    return close


def _block_comment_end(source: str, start: int) -> int:
    depth = 1
    index = start + 2
    while index < len(source) and depth:
        if source.startswith("/*", index):
            depth += 1
            index += 2
        elif source.startswith("*/", index):
            depth -= 1
            index += 2
        else:
            index += 1
    return index


def _blank(result: list[str], start: int, end: int) -> None:
    for index in range(start, end):
        if result[index] != "\n":
            result[index] = " "


def production_code(source: str) -> str:
    masked = mask_non_code(source)
    original = list(masked)
    for match in TEST_MODULE.finditer(masked):
        opening = masked.find("{", match.start(), match.end())
        depth = 0
        end = opening
        for end in range(opening, len(masked)):
            if masked[end] == "{":
                depth += 1
            elif masked[end] == "}":
                depth -= 1
                if depth == 0:
                    end += 1
                    break
        _blank(original, match.start(), end)
    return "".join(original)


def rust_sources() -> list[Path]:
    files = list((ROOT / "runner" / "rust" / "src").rglob("*.rs"))
    files.extend((ROOT / "runner" / "bootstrap-contracts").rglob("*.rs"))
    return sorted(path for path in files if "target" not in path.parts and "tests" not in path.parts)


def main() -> int:
    findings: list[str] = []
    for path in rust_sources():
        text = production_code(path.read_text(encoding="utf-8"))
        for pattern, label in FORBIDDEN:
            for match in pattern.finditer(text):
                line = text.count("\n", 0, match.start()) + 1
                findings.append(f"{path.relative_to(ROOT)}:{line}: {label}")
    if findings:
        print("Rust runtime safety check failed:", file=sys.stderr)
        print("\n".join(findings), file=sys.stderr)
        return 1
    print("Rust runtime safety check passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
