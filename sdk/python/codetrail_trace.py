"""Opt-in, standard-library runtime observations for CodeTrail.

Explicitly select values. No automatic locals, environment, prompts or HTTP headers.
Use asyncio.to_thread in latency-sensitive async apps. Recording failures are visible
warnings by default; strict=True is intended for isolated validation producers.
"""
from __future__ import annotations

import hashlib
import inspect
import json
from pathlib import Path
import re
import subprocess
import sys
import uuid

_SECRET = re.compile(r"api.?key|password|authorization|cookie|secret|access.?token|refresh.?token", re.I)


def selected_values(values):
    """A caller-provided allowlist, with a few known credential names redacted."""
    result = []
    for name, value in values.items():
        text = "[已隐藏敏感字段]" if _SECRET.search(name) else json.dumps(value, ensure_ascii=False, default=str)
        if len(text) > 2000:
            text = text[:1980] + "…[截断]"
        result.append({"name": name, "value": text})
    return result


def location(root, function):
    """Capture a callable's source version BEFORE invoking it, not after return."""
    function = inspect.unwrap(function)
    path = Path(inspect.getsourcefile(function)).resolve()
    relative = path.relative_to(Path(root).resolve()).as_posix()
    lines, start = inspect.getsourcelines(function)
    lines = lines[:200]
    while len("".join(lines).encode("utf-8")) > 16000 and len(lines) > 1:
        lines.pop()
    return {"path": relative, "startLine": start, "endLine": start + len(lines) - 1,
            "fileHash": hashlib.sha256(path.read_bytes()).hexdigest()}


class Recorder:
    def __init__(self, root, cli, label, *, kind="application", producer="Python application", summary, strict=False):
        self.root = str(Path(root).resolve())
        self.cli = str(Path(cli).resolve())
        self.id = uuid.uuid4().hex
        self.strict = strict
        self.errors = []
        self.closed = False
        self.active = self._send("start", {"id": self.id, "label": label, "kind": kind,
                                           "producer": producer, "summary": summary}) is not None

    def _send(self, action, payload):
        try:
            result = subprocess.run([self.cli, "trace", action, self.root], input=json.dumps(payload, ensure_ascii=False),
                                    text=True, encoding="utf-8", capture_output=True, timeout=10, check=False)
            response = json.loads(result.stdout)
            if result.returncode or not response.get("ok"):
                raise RuntimeError(response.get("error", "采集 CLI 未成功"))
            return response["value"]
        except Exception as exc:
            # Never print payloads, variable values, headers or raw application exceptions.
            message = "CodeTrail 采集失败：" + type(exc).__name__
            self.errors.append(message)
            if self.strict:
                raise RuntimeError(message) from exc
            print(message + "；业务继续，运行记录可能不完整", file=sys.stderr)
            return None

    def step(self, title, result, reason, *, source, inputs=None, outputs=None, status="ok", elapsed_ms=0):
        if not self.active or self.closed:
            return None
        return self._send("append", {"runId": self.id, "step": {
            "id": uuid.uuid4().hex, "title": title, "status": status, "result": result, "reason": reason,
            "inputs": selected_values(inputs or {}), "outputs": selected_values(outputs or {}),
            "location": source, "elapsedMs": max(0, int(elapsed_ms))}})

    def finish(self, status, summary, *, verification=None):
        if not self.active or self.closed:
            return None
        payload = {"runId": self.id, "status": status, "summary": summary}
        if verification is not None:
            payload["verification"] = verification
        result = self._send("finish", payload)
        if result is not None:
            self.closed = True
        return result


def verification(goal, *, inputs, expected, actual, key_values=None, verdict="notCompared"):
    """Build an explicit summary from observed values; never execute or infer results."""
    return {"goal": goal, "inputs": selected_values(inputs), "expected": expected,
            "actual": actual, "keyValues": selected_values(key_values or {}), "verdict": verdict}
