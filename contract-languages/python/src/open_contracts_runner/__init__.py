"""Reusable v1 contract bundle reader and runner."""

from .executors import CommandExecutor, ExecutorRegistry, FunctionExecutor, HttpExecutor, TargetExecutor
from .model import Bundle, Case, CaseResult, RunReport, Target
from .reader import BundleReader, CustomReader, JsonYamlReader, Reader, load_bundle, normalize_bundle

__all__ = [
    "Bundle", "BundleReader", "Case", "CaseResult", "CommandExecutor",
    "ContractRunner", "FunctionExecutor",
    "CustomReader", "ExecutorRegistry", "HttpExecutor", "JsonYamlReader",
    "Reader", "RunReport", "Target", "TargetExecutor", "load_bundle",
    "normalize_bundle",
]


def __getattr__(name: str):
    if name == "ContractRunner":
        from .runner import ContractRunner

        return ContractRunner
    raise AttributeError(f"module {__name__!r} has no attribute {name!r}")
