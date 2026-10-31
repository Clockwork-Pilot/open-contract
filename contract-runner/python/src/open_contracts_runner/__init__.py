"""Reusable v1 contract bundle reader and runner."""

from .executors import CommandExecutor, ExecutorRegistry, HttpExecutor, TargetExecutor
from .model import Bundle, Case, CaseResult, Predicate, RunReport, Target
from .reader import BundleReader, CustomReader, JsonYamlReader, Reader, load_bundle, normalize_bundle
from .runner import ContractRunner, CoverageAggregator, CoverageThresholdError

__all__ = [
    "Bundle", "BundleReader", "Case", "CaseResult", "CommandExecutor",
    "ContractRunner", "CoverageAggregator", "CoverageThresholdError",
    "CustomReader", "ExecutorRegistry", "HttpExecutor", "JsonYamlReader",
    "Predicate", "Reader", "RunReport", "Target", "TargetExecutor", "load_bundle",
    "normalize_bundle",
]
