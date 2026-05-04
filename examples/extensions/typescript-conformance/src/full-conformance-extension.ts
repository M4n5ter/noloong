import {
  createStdioJsonRpcExtension,
  type ExtensionContext,
  type JsonObject,
  type JsonValue,
} from "./noloong.ts";

const MODEL_ID = "conformance-model";
const TOOL_NAME = "conformance_echo";
const CONTEXT_ID = "conformance-context";
const PHASE_ID = "conformance.phase";
const PHASE_HOOK_ID = "conformance-hook";
const TOOL_HOOK_ID = "conformance-tool-hook";
const COMPACTION_ID = "conformance-compaction";

createStdioJsonRpcExtension({
  initialize(params) {
    if (params.protocolVersion !== 1) {
      throw new Error("unsupported protocol version");
    }
    return {
      manifest: {
        name: "typescript-conformance-extension",
        version: "0.1.0",
      },
    };
  },

  "capabilities/list"() {
    return {
      capabilities: [
        { type: "model_provider", id: MODEL_ID },
        {
          type: "tool",
          spec: {
            name: TOOL_NAME,
            description: "Echo text from the TypeScript conformance extension",
            inputSchema: {
              type: "object",
              properties: {
                text: { type: "string" },
              },
              required: ["text"],
            },
            permissions: [
              {
                capability: "conformance.echo",
                description: "Allows the conformance echo tool to run",
                metadata: { example: "typescript" },
              },
            ],
          },
        },
        { type: "context_provider", id: CONTEXT_ID },
        { type: "phase_node", id: PHASE_ID },
        { type: "phase_hook", id: PHASE_HOOK_ID },
        { type: "tool_call_hook", id: TOOL_HOOK_ID },
        { type: "compaction_summarizer", id: COMPACTION_ID },
      ],
    };
  },

  "model/stream"(params, context) {
    const streamId = requiredString(params.streamId, "streamId");
    const request = objectValue(params.request);
    const messages = request.messages;

    if (!hasToolResult(messages)) {
      context.streamEvent(streamId, {
        type: "tool_call",
        tool_call: {
          id: "conformance-call-1",
          name: TOOL_NAME,
          arguments: { text: "from model" },
        },
      });
      context.streamEvent(streamId, finishEvent("tool_use"));
      return { streamId };
    }

    context.streamEvent(streamId, { type: "started", stream_id: streamId });
    context.streamEvent(streamId, { type: "text_delta", text: "adapter complete" });
    context.streamEvent(streamId, finishEvent("stop"));
    return { streamId };
  },

  "tool/execute"(params) {
    const request = objectValue(params.request);
    const args = objectValue(request.arguments);
    return {
      content: [{ type: "text", text: String(args.text ?? "") }],
      details: { example: "typescript" },
      isError: false,
      updates: [
        {
          content: [{ type: "text", text: "tool update" }],
          details: { step: 1 },
        },
      ],
    };
  },

  "context/apply"() {
    return {
      effects: [patchContextEffect("conformance_context", true)],
    };
  },

  "phase/run"(params) {
    const request = objectValue(params.request);
    return {
      scratch: objectValue(request.scratch),
      effects: [patchContextEffect("conformance_phase", true)],
      streamEvents: [],
      resolvedToolCalls: [],
      toolOutputs: [],
    };
  },

  "phase_hook/run"(params) {
    requireHook(params, PHASE_HOOK_ID);
    return {};
  },

  "tool_hook/run"(params) {
    requireHook(params, TOOL_HOOK_ID);
    if (params.hookPoint === "before_tool_call") {
      if (stateHasUserText(params, "approval")) {
        return {
          approval: {
            prompt: "Approve TypeScript conformance tool?",
            reason: "TypeScript conformance approval case",
            metadata: { example: "typescript" },
          },
        };
      }
      return {
        decision: {
          outcome: "allow",
          reason: "allowed by TypeScript conformance tool hook",
          approver: "typescript-conformance-extension",
          metadata: { example: "typescript" },
        },
      };
    }
    return {};
  },

  "compaction/summarize"(params) {
    const messages = Array.isArray(params.messagesToSummarize)
      ? params.messagesToSummarize
      : [];
    return {
      summary: `conformance compaction summary: ${messages.length}`,
      metadata: { example: "typescript" },
    };
  },

  shutdown() {
    return {};
  },
}).serve().catch((err: unknown) => {
  process.stderr.write(`${err instanceof Error ? err.message : String(err)}\n`);
  process.exitCode = 1;
});

function finishEvent(stopReason: "stop" | "tool_use"): JsonObject {
  return { type: "finished", stop_reason: stopReason };
}

function hasToolResult(messages: unknown): boolean {
  return Array.isArray(messages) && messages.some((message) => objectValue(message).role === "tool_result");
}

function stateHasUserText(params: JsonObject, expected: string): boolean {
  const state = objectValue(params.state);
  const messages = state.messages;
  if (!Array.isArray(messages)) {
    return false;
  }
  return messages.some((messageValue) => {
    const message = objectValue(messageValue);
    if (message.role !== "user" || !Array.isArray(message.content)) {
      return false;
    }
    return message.content.some((blockValue) => {
      const block = objectValue(blockValue);
      return block.type === "text" && block.text === expected;
    });
  });
}

function requireHook(params: JsonObject, expectedId: string) {
  const hookId = params.hookId;
  if (hookId !== expectedId) {
    throw new Error(`unexpected hook id: ${String(hookId)}`);
  }
}

function requiredString(value: JsonValue | undefined, field: string): string {
  if (typeof value !== "string") {
    throw new Error(`missing string field: ${field}`);
  }
  return value;
}

function objectValue(value: unknown): JsonObject {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? (value as JsonObject)
    : {};
}

function patchContextEffect(key: string, value: JsonValue): JsonObject {
  return {
    type: "patch_context",
    patch: { op: "set", key, value },
  };
}
