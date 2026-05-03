# Noloong Extension Author Guide

本文档是 Noloong process extension bridge 的 wire contract。目标读者是 TypeScript、Python 或其它语言的扩展作者：你应该能只读本文档和示例目录，就实现一个可以被 Rust agent core 加载、运行并通过 conformance 的 stdio JSON-RPC extension。

示例 helper 位于 `examples/extensions/typescript-conformance` 和 `examples/extensions/python-conformance`。它们是 example-local SDK skeleton，不是已发布的稳定 SDK 包。

## Quickstart

TypeScript strict conformance example:

```bash
cd examples/extensions/typescript-conformance
npm install
npm run check
npm run conformance
```

Python strict conformance example:

```bash
python3 -m py_compile examples/extensions/python-conformance/noloong_jsonrpc.py examples/extensions/python-conformance/full_conformance_extension.py
cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile strict -- python3 examples/extensions/python-conformance/full_conformance_extension.py
```

Any extension can run the public conformance CLI:

```bash
cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile hybrid -- node ./extension.mjs
cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile strict --json -- python3 ./extension.py
```

## Transport Rules

- The host starts the extension process with stdin, stdout, and stderr.
- stdin carries one JSON-RPC request per line.
- stdout must carry only one JSON-RPC response or notification per line.
- stderr is available for logs and diagnostics.
- Every host request uses `jsonrpc: "2.0"`, an integer `id`, a `method`, and object `params`.
- Responses must include the same `id` and either `result` or `error`.
- Notifications are only used by the extension for `stream/event` and do not include an `id`.

Example response:

```json
{"jsonrpc":"2.0","id":1,"result":{"manifest":{"name":"example","version":"0.1.0"}}}
```

Example error:

```json
{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"unknown method: model/stream"}}
```

## Wire Conventions

- Named structs use `camelCase`, such as `runId`, `turnId`, and `inputSchema`.
- Tagged enum variants use a `type` field with `snake_case`, such as `text_delta` and `patch_context`.
- Tagged enum payload fields keep their documented wire names. Some are `snake_case`, such as `stream_id`, `stop_reason`, `tool_call`, `tool_call_id`, and `is_error`.
- Context patch variants use an `op` field with `snake_case`, such as `set` and `remove`.
- Optional fields may be omitted. Prefer omitting unknown optional fields over sending `null`.
- Arrays and objects may be empty when the field is optional or documented as no-op.
- A malformed response field fails the active request, stream, or phase.

## Method Index

| Method | Implemented by | Purpose |
|--------|----------------|---------|
| `initialize` | all extensions | Declare manifest and protocol compatibility |
| `capabilities/list` | all extensions | Export providers, hooks, tools, and summarizers |
| `model/stream` | `model_provider` | Produce model stream events |
| `stream/event` | extension notification | Send model stream events asynchronously |
| `tool/execute` | `tool` | Execute a tool call |
| `context/apply` | `context_provider` | Propose context or state effects |
| `phase/run` | `phase_node` | Run a custom phase node |
| `phase_hook/run` | `phase_hook` | Run phase lifecycle hooks |
| `tool_hook/run` | `tool_call_hook` | Run tool permission and output hooks |
| `compaction/summarize` | `compaction_summarizer` | Summarize messages during context compaction |
| `shutdown` | all extensions | Graceful process shutdown |

## Lifecycle Methods

### `initialize`

Called once immediately after the host starts the process.

Params:

```json
{
  "protocolVersion": 1
}
```

Result:

```json
{
  "manifest": {
    "name": "example-extension",
    "version": "0.1.0"
  }
}
```

No-op behavior: none. The extension must return a manifest.

Failure behavior: a JSON-RPC error or malformed result prevents extension startup.

### `capabilities/list`

Called after `initialize`, before runtime registration.

Params:

```json
{}
```

Result:

```json
{
  "capabilities": [
    {"type": "model_provider", "id": "example-model"},
    {"type": "context_provider", "id": "example-context"},
    {"type": "phase_node", "id": "example.phase"},
    {"type": "phase_hook", "id": "example-hook"},
    {"type": "tool_call_hook", "id": "example-tool-hook"},
    {"type": "compaction_summarizer", "id": "example-compaction"},
    {
      "type": "tool",
      "spec": {
        "name": "echo",
        "description": "Echo input text",
        "inputSchema": {
          "type": "object",
          "properties": {
            "text": {"type": "string"}
          },
          "required": ["text"]
        },
        "executionMode": "parallel",
        "permissions": [
          {
            "capability": "example.echo",
            "description": "Allows echo execution",
            "metadata": {"source": "example"}
          }
        ]
      }
    }
  ]
}
```

No-op behavior: returning an empty `capabilities` array is valid but registers nothing useful.

Failure behavior: malformed capability payloads, duplicate provider ids, or duplicate tool names fail runtime registration.

### `shutdown`

Called when the host asks the extension to stop.

Params:

```json
{}
```

Result:

```json
{}
```

No-op behavior: returning `{}` is enough. The process may exit after writing the response.

Failure behavior: shutdown errors are reported to the host but do not define extension capability behavior.

## Model Provider Contract

### `model/stream`

Called when a registered `model_provider` is selected for a model request.

Params:

```json
{
  "providerId": "example-model",
  "streamId": "model-1",
  "request": {
    "runId": "run-1",
    "turnId": 1,
    "messages": [
      {
        "id": "user-1",
        "role": "user",
        "content": [{"type": "text", "text": "hello"}],
        "metadata": {}
      }
    ],
    "context": {},
    "tools": [],
    "metadata": {}
  }
}
```

Normal result:

```json
{
  "streamId": "model-1"
}
```

Inline-events result:

```json
{
  "streamId": "model-1",
  "events": [
    {"type": "started", "stream_id": "model-1"},
    {"type": "text_delta", "text": "hello"},
    {"type": "finished", "stop_reason": "stop"}
  ]
}
```

The extension may stream events before returning by writing `stream/event` notifications to stdout.

Notification:

```json
{
  "jsonrpc": "2.0",
  "method": "stream/event",
  "params": {
    "streamId": "model-1",
    "event": {"type": "text_delta", "text": "hello"}
  }
}
```

No-op behavior: none. A model stream must eventually produce at least one event or complete before the stream timeout. Terminal events are `finished` and `failed`.

Failure behavior:

- A result `streamId` that does not match the request `streamId` fails the stream.
- A malformed event for the active stream fails the stream.
- A `failed` event fails assistant commit if it reaches commit.
- Notifications for unknown stream ids are ignored.

## Tool Provider Contract

### `tool/execute`

Called when a registered tool should execute a resolved tool call.

Params:

```json
{
  "toolName": "echo",
  "request": {
    "runId": "run-1",
    "turnId": 1,
    "toolCallId": "call-1",
    "toolName": "echo",
    "arguments": {"text": "hello"},
    "state": {
      "runId": "run-1",
      "status": "running",
      "messages": [],
      "context": {},
      "availableTools": {},
      "activePhase": "tool.execute",
      "completedTurns": 0,
      "lastError": null
    }
  }
}
```

Result:

```json
{
  "content": [{"type": "text", "text": "hello"}],
  "details": {"source": "example"},
  "isError": false,
  "updates": [
    {
      "content": [{"type": "text", "text": "running"}],
      "details": {"step": 1}
    }
  ]
}
```

No-op behavior: no tool no-op exists. If the tool cannot perform work, return `isError: true` with explanatory content.

Failure behavior: malformed tool outputs become auditable tool errors instead of crashing the whole run. An aborted request still aborts the run.

## Context Provider Contract

### `context/apply`

Called during `context.prepare` for each registered context provider.

Params:

```json
{
  "providerId": "example-context",
  "request": {
    "runId": "run-1",
    "turnId": 1,
    "state": {
      "runId": "run-1",
      "status": "running",
      "messages": [],
      "context": {},
      "availableTools": {},
      "activePhase": "context.prepare",
      "completedTurns": 0,
      "lastError": null
    }
  }
}
```

Result:

```json
{
  "effects": [
    {
      "type": "patch_context",
      "patch": {"op": "set", "key": "example", "value": true}
    }
  ]
}
```

No-op behavior: return `{}` or `{ "effects": [] }`.

Failure behavior: malformed effects fail the current phase.

## Phase Node Contract

### `phase/run`

Called when a registered custom phase node is present in the runtime phase list.

Params:

```json
{
  "phaseId": "example.phase",
  "request": {
    "runId": "run-1",
    "turnId": 1,
    "state": {
      "runId": "run-1",
      "status": "running",
      "messages": [],
      "context": {},
      "availableTools": {},
      "activePhase": "example.phase",
      "completedTurns": 0,
      "lastError": null
    },
    "scratch": {
      "input": null,
      "modelRequest": null,
      "requestMessagesOverride": null,
      "modelEvents": [],
      "assistantMessage": null,
      "toolCalls": [],
      "toolOutputs": [],
      "decision": null
    }
  }
}
```

Result:

```json
{
  "scratch": {
    "modelEvents": [],
    "toolCalls": [],
    "toolOutputs": []
  },
  "effects": [],
  "streamEvents": [],
  "resolvedToolCalls": [],
  "toolOutputs": [],
  "completedToolOutputs": [],
  "toolPermissionAudits": [],
  "completedToolPermissionAudits": []
}
```

No-op behavior: return `{}` or return the incoming `scratch` unchanged.

Failure behavior: malformed phase output fails the current phase.

## Phase Hook Contract

### Common `phase_hook/run` Envelope

All phase hook calls use the same top-level envelope. The hook-specific payload is flattened into the same object.

Common fields:

```json
{
  "hookId": "example-hook",
  "hookPoint": "before_model_request",
  "runId": "run-1",
  "turnId": 1,
  "state": {
    "runId": "run-1",
    "status": "running",
    "messages": [],
    "context": {},
    "availableTools": {},
    "activePhase": "model.request.prepare",
    "completedTurns": 0,
    "lastError": null
  }
}
```

All phase hook results are sparse envelopes. Omitted fields mean no-op.

```json
{}
```

Allowed result fields:

- `modelRequest`
- `modelEvents`
- `assistantMessage`

Malformed optional fields fail the current phase.

### `before_model_request`

Called after core builds a `ModelRequest`, before it is sent to the selected model provider.

Params payload:

```json
{
  "hookPoint": "before_model_request",
  "modelRequest": {
    "runId": "run-1",
    "turnId": 1,
    "messages": [],
    "context": {},
    "tools": [],
    "metadata": {}
  }
}
```

Result that rewrites the request:

```json
{
  "modelRequest": {
    "runId": "run-1",
    "turnId": 1,
    "messages": [],
    "context": {"injected": true},
    "tools": [],
    "metadata": {"hook": "example-hook"}
  }
}
```

No-op behavior: return `{}`.

### `after_model_request`

Called after the model provider finishes streaming and before assistant commit reads the model events.

Params payload:

```json
{
  "hookPoint": "after_model_request",
  "modelRequest": {
    "runId": "run-1",
    "turnId": 1,
    "messages": [],
    "context": {},
    "tools": [],
    "metadata": {}
  },
  "modelEvents": [
    {"type": "text_delta", "text": "hello"},
    {"type": "finished", "stop_reason": "stop"}
  ]
}
```

Result that rewrites events:

```json
{
  "modelEvents": [
    {"type": "text_delta", "text": "rewritten"},
    {"type": "finished", "stop_reason": "stop"}
  ]
}
```

No-op behavior: return `{}`.

### `before_assistant_commit`

Called immediately before model events are converted into an assistant message.

Params payload:

```json
{
  "hookPoint": "before_assistant_commit",
  "modelEvents": [
    {"type": "thinking_delta", "kind": "summary", "textDelta": "short"},
    {"type": "text_delta", "text": "hello"},
    {"type": "finished", "stop_reason": "stop"}
  ]
}
```

Result that rewrites commit source events:

```json
{
  "modelEvents": [
    {"type": "text_delta", "text": "committed text"},
    {"type": "finished", "stop_reason": "stop"}
  ]
}
```

No-op behavior: return `{}`.

### `after_assistant_commit`

Called after core constructs the assistant message and before the append-message effect is emitted.

Params payload:

```json
{
  "hookPoint": "after_assistant_commit",
  "assistantMessage": {
    "id": "assistant-run-1-1",
    "role": "assistant",
    "content": [{"type": "text", "text": "hello"}],
    "metadata": {}
  }
}
```

Result that rewrites the assistant message:

```json
{
  "assistantMessage": {
    "id": "assistant-run-1-1",
    "role": "assistant",
    "content": [{"type": "text", "text": "rewritten"}],
    "metadata": {"hook": "example-hook"}
  }
}
```

No-op behavior: return `{}`.

## Tool Hook Contract

### Common `tool_hook/run` Envelope

All tool hook calls use the same top-level envelope. The hook-specific payload is flattened into the same object.

Common fields:

```json
{
  "hookId": "example-tool-hook",
  "hookPoint": "before_tool_call",
  "runId": "run-1",
  "turnId": 1,
  "state": {
    "runId": "run-1",
    "status": "running",
    "messages": [],
    "context": {},
    "availableTools": {},
    "activePhase": "tool.execute",
    "completedTurns": 0,
    "lastError": null
  }
}
```

### `before_tool_call`

Called before a tool provider executes. It is the permission/approval point.

Params payload:

```json
{
  "hookPoint": "before_tool_call",
  "toolCall": {
    "id": "call-1",
    "name": "echo",
    "arguments": {"text": "hello"}
  },
  "toolSpec": {
    "name": "echo",
    "description": "Echo input text",
    "inputSchema": {},
    "executionMode": "parallel",
    "permissions": [
      {
        "capability": "example.echo",
        "description": "Allows echo execution",
        "metadata": {}
      }
    ]
  },
  "permissions": [
    {
      "capability": "example.echo",
      "description": "Allows echo execution",
      "metadata": {}
    }
  ]
}
```

Result:

```json
{
  "decision": {
    "outcome": "allow",
    "reason": "allowed by example hook",
    "approver": "example-extension",
    "metadata": {"source": "example"}
  }
}
```

No-op behavior: return `{}`. The hook records no decision.

Failure behavior: malformed `decision` fails the tool execution phase. A `deny` outcome is auditable and blocks the tool call.

### `after_tool_call`

Called after a tool provider returns `ToolOutput`.

Params payload:

```json
{
  "hookPoint": "after_tool_call",
  "toolCall": {
    "id": "call-1",
    "name": "echo",
    "arguments": {"text": "hello"}
  },
  "output": {
    "content": [{"type": "text", "text": "hello"}],
    "details": {},
    "isError": false,
    "updates": []
  }
}
```

Result that rewrites the output:

```json
{
  "content": [{"type": "text", "text": "rewritten"}],
  "details": {"hook": "example-tool-hook"},
  "isError": false
}
```

No-op behavior: return `{}`. If all `content`, `details`, and `isError` are omitted, the hook changes nothing.

Failure behavior: malformed rewrite fields fail the tool execution phase.

## Compaction Summarizer Contract

### `compaction/summarize`

Called by `context.compact` when context compaction decides that old messages should be summarized.

Params:

```json
{
  "summarizerId": "example-compaction",
  "runId": "run-1",
  "turnId": 5,
  "previousSummary": "older summary",
  "messagesToSummarize": [
    {
      "id": "user-1",
      "role": "user",
      "content": [{"type": "text", "text": "old question"}],
      "metadata": {}
    }
  ],
  "turnPrefixMessages": [],
  "tokenBudget": 2048,
  "metadata": {}
}
```

Result:

```json
{
  "summary": "summary text",
  "metadata": {"source": "example"}
}
```

No-op behavior: none. `summary` is required and must not be empty after trimming.

Failure behavior: malformed responses or an empty `summary` fail the `context.compact` phase.

## Common JSON Shapes

### `AgentState`

```json
{
  "runId": "run-1",
  "status": "running",
  "messages": [],
  "context": {},
  "availableTools": {},
  "activePhase": "model.stream",
  "completedTurns": 0,
  "lastError": null
}
```

`status` values: `idle`, `running`, `completed`, `aborted`, `failed`.

### `AgentMessage`

```json
{
  "id": "message-1",
  "role": "user",
  "content": [{"type": "text", "text": "hello"}],
  "metadata": {}
}
```

Known roles: `user`, `assistant`, `tool_result`, `system`. Other strings are preserved as custom roles.

### `ContentBlock`

Text:

```json
{"type": "text", "text": "hello"}
```

JSON:

```json
{"type": "json", "value": {"ok": true}}
```

Thinking:

```json
{
  "type": "thinking",
  "thinking": {
    "kind": "summary",
    "text": "short reasoning summary",
    "raw": {"provider": "example", "value": {"tokens": 10}},
    "replayDescriptor": {"id": "thinking-1"},
    "metadata": {}
  }
}
```

Media:

```json
{
  "type": "media",
  "media": {
    "kind": "image",
    "source": {"type": "uri", "uri": "https://example.test/image.png"},
    "mimeType": "image/png",
    "name": "image.png",
    "metadata": {}
  }
}
```

Tool call:

```json
{
  "type": "tool_call",
  "tool_call": {
    "id": "call-1",
    "name": "echo",
    "arguments": {"text": "hello"}
  }
}
```

Tool result:

```json
{
  "type": "tool_result",
  "tool_call_id": "call-1",
  "tool_name": "echo",
  "content": [{"type": "text", "text": "hello"}],
  "is_error": false
}
```

### `ThinkingBlock` and `ThinkingDelta`

Thinking kinds: `raw`, `summary`, `redacted`, `encrypted`, or a custom string.

Stream delta:

```json
{
  "type": "thinking_delta",
  "kind": "summary",
  "textDelta": "short reasoning summary",
  "rawSnapshot": {"provider": "example"},
  "replayDescriptor": {"id": "thinking-1"},
  "metadata": {}
}
```

`text` is accepted as an input alias for `textDelta`, but extensions should emit `textDelta`.

### `MediaBlock` and `MediaDelta`

Media kinds: `file`, `image`, `audio`, `video`, or a custom string.

URI source:

```json
{"type": "uri", "uri": "https://example.test/file.png"}
```

Inline source:

```json
{"type": "inline", "data": "aW1hZ2U=", "encoding": "base64"}
```

Provider source:

```json
{"type": "provider", "providerId": "example-provider", "id": "media-1"}
```

Model stream media delta:

```json
{
  "type": "media_delta",
  "kind": "image",
  "dataDelta": "aW1hZ2U=",
  "mimeType": "image/png",
  "name": "image.png",
  "replayDescriptor": {"id": "media-1"},
  "metadata": {},
  "done": true
}
```

### `ModelStreamEvent`

```json
{"type": "started", "stream_id": "model-1"}
```

```json
{"type": "thinking_delta", "kind": "raw", "textDelta": "thinking"}
```

```json
{"type": "text_delta", "text": "hello"}
```

```json
{"type": "media_delta", "kind": "image", "dataDelta": "aW1hZ2U=", "done": true}
```

```json
{"type": "tool_call", "tool_call": {"id": "call-1", "name": "echo", "arguments": {}}}
```

```json
{"type": "finished", "stop_reason": "stop"}
```

```json
{"type": "failed", "error": "provider error"}
```

Stop reasons: `stop`, `length`, `tool_use`, `error`, `aborted`.

### `ToolSpec`, `ToolCall`, and `ToolOutput`

```json
{
  "name": "echo",
  "description": "Echo input text",
  "inputSchema": {},
  "executionMode": "parallel",
  "permissions": []
}
```

```json
{
  "id": "call-1",
  "name": "echo",
  "arguments": {"text": "hello"}
}
```

```json
{
  "content": [{"type": "text", "text": "hello"}],
  "details": {},
  "isError": false,
  "updates": []
}
```

`executionMode` values: `parallel`, `sequential`.

### `ToolPermissionDecision`

```json
{
  "outcome": "allow",
  "reason": "approved",
  "approver": "example-extension",
  "metadata": {}
}
```

`outcome` values: `allow`, `deny`.

### `AgentEffect`

Append message:

```json
{
  "type": "append_message",
  "message": {
    "id": "message-1",
    "role": "assistant",
    "content": [{"type": "text", "text": "hello"}],
    "metadata": {}
  }
}
```

Patch context:

```json
{
  "type": "patch_context",
  "patch": {"op": "set", "key": "example", "value": true}
}
```

```json
{
  "type": "patch_context",
  "patch": {"op": "remove", "key": "example"}
}
```

Set available tools:

```json
{
  "type": "set_available_tools",
  "tools": []
}
```

Compact messages:

```json
{
  "type": "compact_messages",
  "compaction": {
    "summaryMessage": {
      "id": "summary-1",
      "role": "system",
      "content": [{"type": "text", "text": "summary"}],
      "metadata": {}
    },
    "retainedMessageIds": [],
    "droppedMessageIds": [],
    "tokensBefore": 4096,
    "tokensAfter": 1024,
    "metadata": {}
  }
}
```

### `PhaseScratch` and `PhaseOutput`

`PhaseScratch` carries temporary phase data:

```json
{
  "input": null,
  "modelRequest": null,
  "requestMessagesOverride": null,
  "modelEvents": [],
  "assistantMessage": null,
  "toolCalls": [],
  "toolOutputs": [],
  "decision": null
}
```

`PhaseOutput` may omit fields to use defaults:

```json
{
  "scratch": {},
  "effects": [],
  "streamEvents": [],
  "resolvedToolCalls": [],
  "toolOutputs": [],
  "completedToolOutputs": [],
  "toolPermissionAudits": [],
  "completedToolPermissionAudits": []
}
```

`toolOutputs` and `completedToolOutputs` are arrays of two-item arrays:

```json
[
  [
    {"id": "call-1", "name": "echo", "arguments": {}},
    {"content": [{"type": "text", "text": "ok"}], "details": {}, "isError": false, "updates": []}
  ]
]
```

### `CompactionSummaryRequest` and `CompactionSummaryResult`

The `compaction/summarize` params flatten `CompactionSummaryRequest` and add `summarizerId`.

```json
{
  "runId": "run-1",
  "turnId": 5,
  "previousSummary": null,
  "messagesToSummarize": [],
  "turnPrefixMessages": [],
  "tokenBudget": 2048,
  "metadata": {}
}
```

```json
{
  "summary": "summary text",
  "metadata": {}
}
```

## Conformance Profiles

- `Generic` checks lifecycle, typed capability decode, runtime registration, and shutdown.
- `Hybrid` is the default. It runs generic checks; if no standard conformance capabilities are present, full behavior cases are skipped. Partial standard capability sets fail.
- `Strict` requires all standard conformance capabilities and runs full behavior cases.

Standard strict conformance ids:

- `conformance-model`
- `conformance_echo`
- `conformance-context`
- `conformance.phase`
- `conformance-hook`
- `conformance-tool-hook`
- `conformance-compaction`

Strict behavior requires the extension to exercise model streaming, tool execution, context effects, phase effects, phase hooks, tool call hooks, and compaction summary.

## Minimal Handler Mapping

TypeScript:

```ts
createStdioJsonRpcExtension({
  initialize(params) {
    return { manifest: { name: "example", version: "0.1.0" } };
  },
  "capabilities/list"() {
    return { capabilities: [] };
  },
  "phase_hook/run"(params) {
    if (params.hookPoint === "before_model_request") {
      return {};
    }
    return {};
  },
  shutdown() {
    return {};
  },
}).serve();
```

Python:

```python
def initialize(params, context):
    return {"manifest": {"name": "example", "version": "0.1.0"}}

def list_capabilities(params, context):
    return {"capabilities": []}

def run_phase_hook(params, context):
    if params.get("hookPoint") == "before_model_request":
        return {}
    return {}

serve_extension({
    "initialize": initialize,
    "capabilities/list": list_capabilities,
    "phase_hook/run": run_phase_hook,
    "shutdown": lambda params, context: {},
})
```

## Error Semantics

- JSON-RPC error responses become `AgentCoreError::JsonRpc`.
- Typed payload decode failures become JSON errors and fail the active request or phase.
- Unknown notifications and unrelated stream ids are ignored.
- Unknown response ids do not settle pending requests; the pending request eventually times out or is cancelled.
- stdout close fails all pending requests.
- Tool provider business failures should be returned as `ToolOutput` with `isError: true`.
