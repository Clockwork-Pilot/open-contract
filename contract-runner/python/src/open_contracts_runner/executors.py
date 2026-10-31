"""Target execution interfaces and generic command/HTTP executors."""

from __future__ import annotations

import json
import os
import subprocess
from collections.abc import Callable
from typing import Any, Protocol
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from .model import Case, Target


class TargetExecutor(Protocol):
    def execute(self, target: Target, case: Case) -> Any: ...


class ExecutorRegistry:
    def __init__(self) -> None:
        self._executors: dict[str, TargetExecutor] = {}

    def register(self, kind: str, executor: TargetExecutor) -> None:
        if not kind:
            raise ValueError("executor kind must be non-empty")
        if kind in self._executors:
            raise ValueError(f"executor already registered for target kind: {kind}")
        self._executors[kind] = executor

    def get(self, kind: str) -> TargetExecutor:
        try:
            return self._executors[kind]
        except KeyError as exc:
            raise ValueError(f"no executor registered for target kind: {kind}") from exc

    @classmethod
    def with_builtins(cls) -> "ExecutorRegistry":
        registry = cls()
        command = CommandExecutor()
        registry.register("command", command)
        registry.register("hooks", command)
        registry.register("http", HttpExecutor())
        return registry


class CommandExecutor:
    """Run one configured command per case, passing its input JSON on stdin."""

    def execute(self, target: Target, case: Case) -> Any:
        command = target.config.get("command")
        if not isinstance(command, list) or not command or not all(isinstance(arg, str) for arg in command):
            raise ValueError(f"target {target.id} config.command must be a non-empty string array")
        environment = os.environ.copy()
        configured_env = target.config.get("env", {})
        if not isinstance(configured_env, dict) or not all(
            isinstance(key, str) and isinstance(value, str) for key, value in configured_env.items()
        ):
            raise ValueError(f"target {target.id} config.env must map strings to strings")
        environment.update(configured_env)
        completed = subprocess.run(
            command,
            input=json.dumps(case.input, ensure_ascii=False),
            text=True,
            capture_output=True,
            cwd=target.config.get("cwd"),
            env=environment,
            timeout=target.config.get("timeout", 30),
            check=False,
        )
        if completed.returncode:
            raise RuntimeError(
                f"command exited {completed.returncode}: {completed.stderr.strip()}"
            )
        try:
            return json.loads(completed.stdout)
        except json.JSONDecodeError:
            return completed.stdout


class FunctionExecutor:
    """Call a Python evaluator once per case with the case input."""

    def __init__(self, evaluator: Callable[[Any], Any]) -> None:
        if not callable(evaluator):
            raise TypeError("evaluator must be callable")
        self.evaluator = evaluator

    def execute(self, target: Target, case: Case) -> Any:
        return self.evaluator(case.input)


class HttpExecutor:
    """POST case input as JSON and normalize the status and response body."""

    def execute(self, target: Target, case: Case) -> Any:
        url = target.config.get("url")
        if not isinstance(url, str) or not url:
            raise ValueError(f"target {target.id} config.url must be a non-empty string")
        method = target.config.get("method", "POST")
        headers = target.config.get("headers", {})
        timeout = target.config.get("timeout", 30)
        if not isinstance(method, str) or not isinstance(headers, dict):
            raise ValueError(f"target {target.id} method must be a string and headers an object")
        request_headers = {"Content-Type": "application/json", **headers}
        request = Request(
            url,
            data=json.dumps(case.input, ensure_ascii=False).encode("utf-8"),
            headers=request_headers,
            method=method.upper(),
        )
        try:
            with urlopen(request, timeout=timeout) as response:
                status = response.status
                body = response.read().decode("utf-8")
        except HTTPError as response:
            status = response.code
            body = response.read().decode("utf-8")
        try:
            normalized_body = json.loads(body)
        except json.JSONDecodeError:
            normalized_body = body
        return {"status": status, "body": normalized_body}
