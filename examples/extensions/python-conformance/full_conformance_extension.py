from __future__ import annotations

from typing import Any

from noloong_jsonrpc import ExtensionContext, JsonDict, serve_extension

MODEL_ID = "conformance-model"
TOOL_NAME = "conformance_echo"
CONTEXT_ID = "conformance-context"
PHASE_ID = "conformance.phase"
PHASE_HOOK_ID = "conformance-hook"
TOOL_HOOK_ID = "conformance-tool-hook"
COMPACTION_ID = "conformance-compaction"


def initialize(params: JsonDict, _context: ExtensionContext) -> JsonDict:
    if params.get("protocolVersion") != 1:
        raise ValueError("unsupported protocol version")
    return {"manifest": {"name": "python-conformance-extension", "version": "0.1.0"}}


def list_capabilities(_params: JsonDict, _context: ExtensionContext) -> JsonDict:
    return {
        "capabilities": [
            {"type": "model_provider", "id": MODEL_ID},
            {
                "type": "tool",
                "spec": {
                    "name": TOOL_NAME,
                    "description": "Echo text from the Python conformance extension",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"text": {"type": "string"}},
                        "required": ["text"],
                    },
                    "permissions": [
                        {
                            "capability": "conformance.echo",
                            "description": "Allows the conformance echo tool to run",
                            "metadata": {"example": "python"},
                        }
                    ],
                },
            },
            {"type": "context_provider", "id": CONTEXT_ID},
            {"type": "phase_node", "id": PHASE_ID},
            {"type": "phase_hook", "id": PHASE_HOOK_ID},
            {"type": "tool_call_hook", "id": TOOL_HOOK_ID},
            {"type": "compaction_summarizer", "id": COMPACTION_ID},
        ]
    }


def stream_model(params: JsonDict, context: ExtensionContext) -> JsonDict:
    stream_id = required_string(params.get("streamId"), "streamId")
    request = record(params.get("request"))
    messages = request.get("messages")

    if not has_tool_result(messages):
        context.stream_event(
            stream_id,
            {
                "type": "tool_call",
                "tool_call": {
                    "id": "conformance-call-1",
                    "name": TOOL_NAME,
                    "arguments": {"text": "from model"},
                },
            },
        )
        context.stream_event(stream_id, finish_event("tool_use"))
        return {"streamId": stream_id}

    context.stream_event(stream_id, {"type": "started", "stream_id": stream_id})
    context.stream_event(stream_id, {"type": "text_delta", "text": "adapter complete"})
    context.stream_event(stream_id, finish_event("stop"))
    return {"streamId": stream_id}


def execute_tool(params: JsonDict, _context: ExtensionContext) -> JsonDict:
    request = record(params.get("request"))
    arguments = record(request.get("arguments"))
    return {
        "content": [{"type": "text", "text": str(arguments.get("text", ""))}],
        "details": {"example": "python"},
        "isError": False,
        "updates": [
            {
                "content": [{"type": "text", "text": "tool update"}],
                "details": {"step": 1},
            }
        ],
    }


def apply_context(_params: JsonDict, _context: ExtensionContext) -> JsonDict:
    return {
        "effects": [patch_context_effect("conformance_context", True)]
    }


def run_phase(params: JsonDict, _context: ExtensionContext) -> JsonDict:
    request = record(params.get("request"))
    return {
        "scratch": record(request.get("scratch")),
        "effects": [patch_context_effect("conformance_phase", True)],
        "streamEvents": [],
        "resolvedToolCalls": [],
        "toolOutputs": [],
    }


def run_phase_hook(params: JsonDict, _context: ExtensionContext) -> JsonDict:
    require_hook(params, PHASE_HOOK_ID)
    return {}


def run_tool_hook(params: JsonDict, _context: ExtensionContext) -> JsonDict:
    require_hook(params, TOOL_HOOK_ID)
    if params.get("hookPoint") == "before_tool_call":
        return {
            "decision": {
                "outcome": "allow",
                "reason": "allowed by Python conformance tool hook",
                "approver": "python-conformance-extension",
                "metadata": {"example": "python"},
            }
        }
    return {}


def summarize_compaction(params: JsonDict, _context: ExtensionContext) -> JsonDict:
    messages = params.get("messagesToSummarize")
    count = len(messages) if isinstance(messages, list) else 0
    return {
        "summary": f"conformance compaction summary: {count}",
        "metadata": {"example": "python"},
    }


def shutdown(_params: JsonDict, _context: ExtensionContext) -> JsonDict:
    return {}


def finish_event(stop_reason: str) -> JsonDict:
    return {"type": "finished", "stop_reason": stop_reason}


def has_tool_result(messages: Any) -> bool:
    return isinstance(messages, list) and any(record(message).get("role") == "tool_result" for message in messages)


def require_hook(params: JsonDict, expected_id: str) -> None:
    if params.get("hookId") != expected_id:
        raise ValueError(f"unexpected hook id: {params.get('hookId')}")


def required_string(value: Any, field: str) -> str:
    if not isinstance(value, str):
        raise ValueError(f"missing string field: {field}")
    return value


def record(value: Any) -> JsonDict:
    return value if isinstance(value, dict) else {}


def patch_context_effect(key: str, value: Any) -> JsonDict:
    return {
        "type": "patch_context",
        "patch": {"op": "set", "key": key, "value": value},
    }


if __name__ == "__main__":
    serve_extension(
        {
            "initialize": initialize,
            "capabilities/list": list_capabilities,
            "model/stream": stream_model,
            "tool/execute": execute_tool,
            "context/apply": apply_context,
            "phase/run": run_phase,
            "phase_hook/run": run_phase_hook,
            "tool_hook/run": run_tool_hook,
            "compaction/summarize": summarize_compaction,
            "shutdown": shutdown,
        }
    )
