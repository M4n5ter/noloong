# Python Conformance Extension

This example shows a minimal Python stdio JSON-RPC extension for Noloong. It uses only the Python standard library. The helper in `noloong_jsonrpc.py` is an example SDK skeleton, not a published API package.

The full wire contract lives in `../../../crates/noloong-agent-core/docs/EXTENSIONS.md`. This directory is the smallest Python implementation that exercises every standard conformance capability.

Compile-check the example:

```bash
python3 -m py_compile noloong_jsonrpc.py full_conformance_extension.py
```

Run strict conformance from the repository root:

```bash
cargo run -p noloong-agent-core --bin noloong-extension-conformance -- --profile strict -- python3 examples/extensions/python-conformance/full_conformance_extension.py
```

Start the extension manually:

```bash
python3 full_conformance_extension.py
```

## Handler Mapping

`full_conformance_extension.py` passes a method table to `serve_extension(...)`:

| Function | JSON-RPC method | Contract |
|----------|-----------------|----------|
| `initialize` | `initialize` | Receives `protocolVersion`; returns `manifest`. |
| `list_capabilities` | `capabilities/list` | Returns all exported capabilities. |
| `stream_model` | `model/stream` | Receives `providerId`, `streamId`, and `request`; sends `stream/event` notifications through the helper. |
| `execute_tool` | `tool/execute` | Receives `toolName` and `request`; returns `ToolOutput`. |
| `apply_context` | `context/apply` | Receives `providerId` and `request`; returns context effects. |
| `run_phase` | `phase/run` | Receives `phaseId` and `request`; returns `PhaseOutput`. |
| `run_phase_hook` | `phase_hook/run` | Receives `hookId`, `hookPoint`, common state fields, and hook-specific payload. |
| `run_tool_hook` | `tool_hook/run` | Receives `hookId`, `hookPoint`, common state fields, and tool-specific payload. |
| `summarize_compaction` | `compaction/summarize` | Receives flattened compaction request fields plus `summarizerId`; returns `summary`. |
| `shutdown` | `shutdown` | Returns `{}`. |

`run_tool_hook` demonstrates both permission decisions and human approval requests. The normal conformance path returns an allow `decision`; the approval conformance path returns `approval`, causing core to pause the run and later replay the human `ToolApprovalResolution` into the standard permission audit.

The helper writes only JSON-RPC messages to stdout. Use stderr for diagnostics. For model streaming, call `context.stream_event(stream_id, event)` from the `model/stream` handler; the helper emits the `stream/event` notification shape required by the bridge.
