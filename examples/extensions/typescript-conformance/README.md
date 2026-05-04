# TypeScript Conformance Extension

This example shows a minimal TypeScript stdio JSON-RPC extension for Noloong. It uses a local helper in `src/noloong.ts`; the helper is an example SDK skeleton, not a published API package.

The full wire contract lives in `../../../crates/noloong-agent-core/docs/EXTENSIONS.md`. This directory is the smallest TypeScript implementation that exercises every standard conformance capability.

Run it from this directory:

```bash
npm install
npm run check
npm run conformance
```

Start the extension manually:

```bash
npm run start
```

The extension implements the full standard conformance capability set and should pass `noloong-extension-conformance --profile strict`.

## Handler Mapping

`src/full-conformance-extension.ts` registers handlers by JSON-RPC method name:

| Handler key | Contract |
|-------------|----------|
| `initialize` | Receives `protocolVersion`; returns `manifest`. |
| `capabilities/list` | Returns all exported capabilities. |
| `model/stream` | Receives `providerId`, `streamId`, and `request`; sends `stream/event` notifications through the helper. |
| `tool/execute` | Receives `toolName` and `request`; returns `ToolOutput`. |
| `context/apply` | Receives `providerId` and `request`; returns context effects. |
| `phase/run` | Receives `phaseId` and `request`; returns `PhaseOutput`. |
| `phase_hook/run` | Receives `hookId`, `hookPoint`, common state fields, and hook-specific payload. |
| `tool_hook/run` | Receives `hookId`, `hookPoint`, common state fields, and tool-specific payload. |
| `compaction/summarize` | Receives flattened compaction request fields plus `summarizerId`; returns `summary`. |
| `shutdown` | Returns `{}`. |

`tool_hook/run` demonstrates both permission decisions and human approval requests. The normal conformance path returns an allow `decision`; the approval conformance path returns `approval`, causing core to pause the run and later replay the human `ToolApprovalResolution` into the standard permission audit.

The helper writes only JSON-RPC messages to stdout. Use stderr for diagnostics. For model streaming, call `context.streamEvent(streamId, event)` from the `model/stream` handler; the helper emits the `stream/event` notification shape required by the bridge.
