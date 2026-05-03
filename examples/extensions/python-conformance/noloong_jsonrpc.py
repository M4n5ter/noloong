from __future__ import annotations

import json
import sys
from collections.abc import Callable
from dataclasses import dataclass
from typing import Any

JsonDict = dict[str, Any]
Handler = Callable[[JsonDict, "ExtensionContext"], Any]


@dataclass(frozen=True)
class ExtensionContext:
    def stream_event(self, stream_id: str, event: JsonDict) -> None:
        send(
            {
                "jsonrpc": "2.0",
                "method": "stream/event",
                "params": {"streamId": stream_id, "event": event},
            }
        )

    def log(self, message: str) -> None:
        sys.stderr.write(f"{message}\n")
        sys.stderr.flush()


def serve_extension(handlers: dict[str, Handler]) -> None:
    context = ExtensionContext()
    for line in sys.stdin:
        if not line.strip():
            continue
        if handle_line(line, handlers, context):
            break


def handle_line(line: str, handlers: dict[str, Handler], context: ExtensionContext) -> bool:
    try:
        request = json.loads(line)
    except json.JSONDecodeError:
        error(None, -32700, "parse error")
        return False

    if not isinstance(request, dict):
        error(None, -32600, "invalid request")
        return False

    request_id = read_id(request.get("id"))
    method = request.get("method")
    if request.get("jsonrpc") != "2.0" or not isinstance(method, str):
        error(request_id, -32600, "invalid request")
        return False

    handler = handlers.get(method)
    if handler is None:
        error(request_id, -32601, f"unknown method: {method}")
        return False

    try:
        params = request.get("params")
        value = handler(params if isinstance(params, dict) else {}, context)
        result(request_id, value if value is not None else {})
        return method == "shutdown"
    except Exception as exc:
        context.log(str(exc))
        error(request_id, -32000, "handler failed")
        return False


def send(message: JsonDict) -> None:
    sys.stdout.write(json.dumps(message, separators=(",", ":")) + "\n")
    sys.stdout.flush()


def result(request_id: str | int | None, value: Any) -> None:
    send({"jsonrpc": "2.0", "id": request_id, "result": value})


def error(request_id: str | int | None, code: int, message: str) -> None:
    send({"jsonrpc": "2.0", "id": request_id, "error": {"code": code, "message": message}})


def read_id(value: Any) -> str | int | None:
    return value if isinstance(value, str | int) or value is None else None
