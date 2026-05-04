#!/usr/bin/env node

import readline from "node:readline";

const modes = new Set(
  process.argv
    .filter((arg) => arg.startsWith("--mode="))
    .flatMap((arg) => arg.slice("--mode=".length).split(","))
    .filter(Boolean),
);

const Mode = Object.freeze({
  allCapabilities: "all-capabilities",
  adapterPayloads: "adapter-payloads",
  delayedStream: "delayed-stream",
  duplicateCompaction: "duplicate-compaction",
  duplicateContext: "duplicate-context",
  duplicateModel: "duplicate-model",
  duplicatePhase: "duplicate-phase",
  duplicatePhaseHook: "duplicate-phase-hook",
  duplicateToolCallHook: "duplicate-tool-call-hook",
  duplicateTool: "duplicate-tool",
  invalidStreamResult: "invalid-stream-result",
  lateResponseAfterCancel: "late-response-after-cancel",
  malformedActiveStream: "malformed-active-stream",
  malformedCapabilities: "malformed-capabilities",
  malformedCompactionResult: "malformed-compaction-result",
  malformedContextResult: "malformed-context-result",
  malformedManifest: "malformed-manifest",
  malformedPhaseHookResult: "malformed-phase-hook-result",
  malformedPhaseResult: "malformed-phase-result",
  malformedToolHookResult: "malformed-tool-hook-result",
  malformedToolResult: "malformed-tool-result",
  missingResult: "missing-result",
  modelJsonrpcError: "model-jsonrpc-error",
  partialConformance: "partial-conformance",
  responseBufferedEvents: "response-buffered-events",
  stdoutClose: "stdout-close",
  streamHangs: "stream-hangs",
  streamNoResponse: "stream-no-response",
  toolHookDeny: "tool-hook-deny",
  toolHookPayloads: "tool-hook-payloads",
  unknownCapability: "unknown-capability",
  unknownStreamNotification: "unknown-stream-notification",
  wrongResponseId: "wrong-response-id",
});

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

function hasMode(mode) {
  return modes.has(mode);
}

function stateHasUserText(params, expected) {
  return (params.state?.messages ?? []).some((message) =>
    message.role === "user" &&
    (message.content ?? []).some((block) => block.type === "text" && block.text === expected),
  );
}

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function result(id, value) {
  send({ jsonrpc: "2.0", id, result: value });
}

function error(id, message, code = -32000) {
  send({ jsonrpc: "2.0", id, error: { code, message } });
}

function responseWithoutResult(id) {
  send({ jsonrpc: "2.0", id });
}

function stream(streamId, event) {
  send({
    jsonrpc: "2.0",
    method: "stream/event",
    params: { streamId, event },
  });
}

function assertOrError(id, condition, message) {
  if (!condition) {
    error(id, message);
    return false;
  }
  return true;
}

function textEvent(text) {
  return { type: "text_delta", text };
}

function finishEvent(stopReason = "stop") {
  return { type: "finished", stop_reason: stopReason };
}

function allCapabilities() {
  return [
    { type: "model_provider", id: "conformance-model" },
    {
      type: "tool",
      spec: {
        name: "conformance_echo",
        description: "Echo text from the JSON-RPC conformance fixture",
        inputSchema: {
          type: "object",
          properties: { text: { type: "string" } },
          required: ["text"],
        },
        permissions: [
          {
            capability: "conformance.echo",
            description: "Allows the conformance echo tool to run",
            metadata: { fixture: "permission" },
          },
        ],
      },
    },
    { type: "context_provider", id: "conformance-context" },
    { type: "phase_node", id: "conformance.phase" },
    { type: "phase_hook", id: "conformance-hook" },
    { type: "tool_call_hook", id: "conformance-tool-hook" },
    { type: "compaction_summarizer", id: "conformance-compaction" },
  ];
}

function modelOnlyCapabilities() {
  return [{ type: "model_provider", id: "conformance-model" }];
}

function capabilitiesForMode() {
  for (const [mode, capabilities] of duplicateCapabilityCases()) {
    if (hasMode(mode)) {
      return capabilities;
    }
  }
  if (hasMode(Mode.partialConformance)) {
    return [allCapabilities()[0], allCapabilities()[1]];
  }
  if (hasMode(Mode.allCapabilities)) {
    return allCapabilities();
  }
  return modelOnlyCapabilities();
}

function toolSpec(name) {
  return {
    name,
    description: "Duplicate test tool",
    inputSchema: { type: "object" },
  };
}

function duplicateCapabilityCases() {
  return [
    [
      Mode.duplicateModel,
      [
        { type: "model_provider", id: "duplicate-model" },
        { type: "model_provider", id: "duplicate-model" },
      ],
    ],
    [
      Mode.duplicateTool,
      [
        { type: "tool", spec: toolSpec("duplicate_tool") },
        { type: "tool", spec: toolSpec("duplicate_tool") },
      ],
    ],
    [
      Mode.duplicateContext,
      [
        { type: "context_provider", id: "duplicate-context" },
        { type: "context_provider", id: "duplicate-context" },
      ],
    ],
    [
      Mode.duplicatePhase,
      [
        { type: "phase_node", id: "duplicate.phase" },
        { type: "phase_node", id: "duplicate.phase" },
      ],
    ],
    [
      Mode.duplicatePhaseHook,
      [
        { type: "phase_hook", id: "duplicate-hook" },
        { type: "phase_hook", id: "duplicate-hook" },
      ],
    ],
    [
      Mode.duplicateToolCallHook,
      [
        { type: "tool_call_hook", id: "duplicate-tool-hook" },
        { type: "tool_call_hook", id: "duplicate-tool-hook" },
      ],
    ],
    [
      Mode.duplicateCompaction,
      [
        { type: "compaction_summarizer", id: "duplicate-compaction" },
        { type: "compaction_summarizer", id: "duplicate-compaction" },
      ],
    ],
  ];
}

function assertModelStreamParams(id, params) {
  const echoTool = (params.request?.tools ?? []).find((tool) => tool.name === "conformance_echo");
  return (
    assertOrError(id, params.providerId === "conformance-model", "model providerId mismatch") &&
    assertOrError(id, typeof params.streamId === "string", "model streamId missing") &&
    assertOrError(id, params.request?.runId === "run-1", "model request runId mismatch") &&
    assertOrError(id, Number.isInteger(params.request?.turnId), "model request turnId missing") &&
    assertOrError(id, Array.isArray(params.request?.messages), "model request messages missing") &&
    assertOrError(id, Array.isArray(params.request?.tools), "model request tools missing") &&
    (!echoTool ||
      assertOrError(
        id,
        echoTool.permissions?.[0]?.capability === "conformance.echo",
        "model request tool permissions missing",
      ))
  );
}

function assertToolParams(id, params) {
  return (
    assertOrError(id, params.toolName === "conformance_echo", "toolName mismatch") &&
    assertOrError(id, params.request?.toolName === "conformance_echo", "tool request toolName mismatch") &&
    assertOrError(id, params.request?.toolCallId === "conformance-call-1", "toolCallId mismatch") &&
    assertOrError(id, params.request?.arguments?.text === "from model", "tool arguments mismatch") &&
    assertOrError(id, params.request?.state?.messages?.length > 0, "tool request state missing")
  );
}

function assertContextParams(id, params) {
  return (
    assertOrError(id, params.providerId === "conformance-context", "context providerId mismatch") &&
    assertOrError(id, params.request?.runId === "run-1", "context request runId mismatch") &&
    assertOrError(id, Number.isInteger(params.request?.turnId), "context turnId missing") &&
    assertOrError(id, params.request?.state, "context state missing")
  );
}

function assertPhaseParams(id, params) {
  return (
    assertOrError(id, params.phaseId === "conformance.phase", "phaseId mismatch") &&
    assertOrError(id, params.request?.runId === "run-1", "phase request runId mismatch") &&
    assertOrError(id, Number.isInteger(params.request?.turnId), "phase turnId missing") &&
    assertOrError(id, params.request?.state, "phase state missing") &&
    assertOrError(id, params.request?.scratch, "phase scratch missing")
  );
}

function assertPhaseHookParams(id, params) {
  if (
    !assertOrError(id, params.hookId === "conformance-hook", "hookId mismatch") ||
    !assertOrError(id, params.runId === "run-1", "hook runId mismatch") ||
    !assertOrError(id, Number.isInteger(params.turnId), "hook turnId missing") ||
    !assertOrError(id, params.state, "hook state missing")
  ) {
    return false;
  }
  if (params.hookPoint === "before_model_request") {
    return assertOrError(id, params.modelRequest?.messages, "before_model_request payload missing");
  }
  if (params.hookPoint === "after_model_request") {
    return (
      assertOrError(id, params.modelRequest?.messages, "after_model_request modelRequest missing") &&
      assertOrError(id, Array.isArray(params.modelEvents), "after_model_request modelEvents missing")
    );
  }
  if (params.hookPoint === "before_assistant_commit") {
    return assertOrError(id, Array.isArray(params.modelEvents), "before_assistant_commit modelEvents missing");
  }
  if (params.hookPoint === "after_assistant_commit") {
    return assertOrError(id, params.assistantMessage?.role === "assistant", "after_assistant_commit assistant missing");
  }
  return assertOrError(id, false, `unexpected hook point: ${params.hookPoint}`);
}

function assertToolHookParams(id, params) {
  if (
    !assertOrError(id, params.hookId === "conformance-tool-hook", "tool hookId mismatch") ||
    !assertOrError(id, params.runId === "run-1", "tool hook runId mismatch") ||
    !assertOrError(id, Number.isInteger(params.turnId), "tool hook turnId missing") ||
    !assertOrError(id, params.state, "tool hook state missing") ||
    !assertOrError(id, params.toolCall?.name === "conformance_echo", "tool hook toolCall mismatch")
  ) {
    return false;
  }
  if (params.hookPoint === "before_tool_call") {
    return (
      assertOrError(id, params.toolCall?.arguments?.text === "from model", "tool hook arguments mismatch") &&
      assertOrError(id, params.toolSpec?.name === "conformance_echo", "tool hook toolSpec mismatch") &&
      assertOrError(
        id,
        params.permissions?.[0]?.capability === "conformance.echo",
        "tool hook permissions missing",
      )
    );
  }
  if (params.hookPoint === "after_tool_call") {
    if (hasMode(Mode.toolHookPayloads)) {
      return assertOrError(id, params.output?.content?.[0]?.text === "from model", "tool hook output missing");
    }
    return assertOrError(id, params.output, "tool hook output missing");
  }
  return assertOrError(id, false, `unexpected tool hook point: ${params.hookPoint}`);
}

function assertCompactionParams(id, params) {
  return (
    assertOrError(id, params.summarizerId === "conformance-compaction", "summarizerId mismatch") &&
    assertOrError(id, params.runId === "run-1", "compaction runId mismatch") &&
    assertOrError(id, Number.isInteger(params.turnId), "compaction turnId missing") &&
    assertOrError(id, Array.isArray(params.messagesToSummarize), "messagesToSummarize missing") &&
    assertOrError(id, Array.isArray(params.turnPrefixMessages), "turnPrefixMessages missing") &&
    assertOrError(id, Number.isInteger(params.tokenBudget), "tokenBudget missing") &&
    assertOrError(id, params.metadata, "metadata missing")
  );
}

function hasToolResult(messages) {
  return (messages ?? []).some((message) => message.role === "tool_result");
}

for await (const line of rl) {
  if (!line.trim()) continue;
  const request = JSON.parse(line);
  const { id, method, params } = request;

  if (method === "initialize") {
    if (!assertOrError(id, request.jsonrpc === "2.0", "jsonrpc version mismatch")) continue;
    if (!assertOrError(id, params?.protocolVersion === 1, "protocolVersion mismatch")) continue;
    if (hasMode(Mode.malformedManifest)) {
      result(id, { manifest: { name: 42, version: "0.1.0" } });
      continue;
    }
    result(id, {
      manifest: { name: "jsonrpc-conformance-fixture", version: "0.1.0" },
    });
    continue;
  }

  if (method === "capabilities/list") {
    if (hasMode(Mode.malformedCapabilities)) {
      result(id, { capabilities: "not an array" });
      continue;
    }
    if (hasMode(Mode.unknownCapability)) {
      result(id, { capabilities: [{ type: "unknown_capability", id: "unknown" }] });
      continue;
    }
    result(id, { capabilities: capabilitiesForMode() });
    continue;
  }

  if (method === "context/apply") {
    if (!assertContextParams(id, params)) continue;
    if (hasMode(Mode.malformedContextResult)) {
      result(id, { effects: "not an array" });
      continue;
    }
    result(id, {
      effects: [
        {
          type: "patch_context",
          patch: { op: "set", key: "conformance_context", value: true },
        },
      ],
    });
    continue;
  }

  if (method === "model/stream") {
    if (!assertModelStreamParams(id, params)) continue;
    const streamId = params.streamId;
    if (hasMode(Mode.modelJsonrpcError)) {
      error(id, "fixture model jsonrpc error");
      continue;
    }
    if (hasMode(Mode.wrongResponseId)) {
      result(id + 1000, { streamId });
      continue;
    }
    if (hasMode(Mode.missingResult)) {
      responseWithoutResult(id);
      continue;
    }
    if (hasMode(Mode.invalidStreamResult)) {
      result(id, { streamId: 42 });
      continue;
    }
    if (hasMode(Mode.stdoutClose)) {
      process.exit(0);
    }
    if (hasMode(Mode.lateResponseAfterCancel)) {
      setTimeout(() => result(id, { streamId, events: [textEvent("late"), finishEvent()] }), 150);
      continue;
    }
    if (hasMode(Mode.responseBufferedEvents)) {
      result(id, {
        streamId,
        events: [
          { type: "started", stream_id: streamId },
          textEvent("buffered response"),
          finishEvent(),
        ],
      });
      continue;
    }
    if (hasMode(Mode.delayedStream)) {
      stream(streamId, { type: "started", stream_id: streamId });
      stream(streamId, textEvent("delayed chunk"));
      await new Promise((resolve) => setTimeout(resolve, 150));
      stream(streamId, finishEvent());
      result(id, { streamId });
      continue;
    }
    if (hasMode(Mode.streamNoResponse)) {
      stream(streamId, { type: "started", stream_id: streamId });
      stream(streamId, textEvent("terminal chunk"));
      stream(streamId, finishEvent());
      continue;
    }
    if (hasMode(Mode.streamHangs)) {
      stream(streamId, { type: "started", stream_id: streamId });
      stream(streamId, textEvent("hanging chunk"));
      continue;
    }
    if (hasMode(Mode.malformedActiveStream)) {
      stream(streamId, { type: "text_delta", text: 42 });
      result(id, { streamId });
      continue;
    }
    if (hasMode(Mode.unknownStreamNotification)) {
      stream("unknown-stream-id", textEvent("ignored"));
      stream(streamId, textEvent("unknown stream ok"));
      stream(streamId, finishEvent());
      result(id, { streamId });
      continue;
    }
    if (hasMode(Mode.adapterPayloads) && !hasToolResult(params.request.messages)) {
      stream(streamId, {
        type: "tool_call",
        tool_call: {
          id: "conformance-call-1",
          name: "conformance_echo",
          arguments: { text: "from model" },
        },
      });
      stream(streamId, finishEvent("tool_use"));
      result(id, { streamId });
      continue;
    }
    const text = hasMode(Mode.adapterPayloads) ? "adapter complete" : "model ok";
    stream(streamId, { type: "started", stream_id: streamId });
    stream(streamId, textEvent(text));
    stream(streamId, finishEvent());
    result(id, { streamId });
    continue;
  }

  if (method === "tool/execute") {
    if (!assertToolParams(id, params)) continue;
    if (hasMode(Mode.malformedToolResult)) {
      result(id, { content: "not an array" });
      continue;
    }
    result(id, {
      content: [{ type: "text", text: params.request.arguments.text }],
      details: { fixture: "tool" },
      isError: false,
      updates: [
        {
          content: [{ type: "text", text: "tool update" }],
          details: { step: 1 },
        },
      ],
    });
    continue;
  }

  if (method === "tool_hook/run") {
    if (!assertToolHookParams(id, params)) continue;
    if (hasMode(Mode.malformedToolHookResult)) {
      result(id, { decision: "not an object" });
      continue;
    }
    if (params.hookPoint === "before_tool_call" && hasMode(Mode.toolHookDeny)) {
      result(id, {
        decision: {
          outcome: "deny",
          reason: "denied by conformance tool hook",
          approver: "jsonrpc-fixture",
          metadata: { fixture: "tool-hook-deny" },
        },
      });
      continue;
    }
    if (params.hookPoint === "before_tool_call" && stateHasUserText(params, "approval")) {
      result(id, {
        approval: {
          prompt: "Approve conformance tool?",
          reason: "conformance approval case",
          metadata: { fixture: "tool-hook-approval" },
        },
      });
      continue;
    }
    if (params.hookPoint === "before_tool_call" && hasMode(Mode.toolHookPayloads)) {
      result(id, {
        decision: {
          outcome: "allow",
          reason: "allowed by conformance tool hook",
          approver: "jsonrpc-fixture",
          metadata: { fixture: "tool-hook-allow" },
        },
      });
      continue;
    }
    result(id, {});
    continue;
  }

  if (method === "phase/run") {
    if (!assertPhaseParams(id, params)) continue;
    if (hasMode(Mode.malformedPhaseResult)) {
      result(id, { scratch: "not an object" });
      continue;
    }
    result(id, {
      scratch: params.request.scratch,
      effects: [
        {
          type: "patch_context",
          patch: { op: "set", key: "conformance_phase", value: true },
        },
      ],
      streamEvents: [],
      resolvedToolCalls: [],
      toolOutputs: [],
    });
    continue;
  }

  if (method === "phase_hook/run") {
    if (!assertPhaseHookParams(id, params)) continue;
    if (hasMode(Mode.malformedPhaseHookResult)) {
      result(id, { modelRequest: "not an object" });
      continue;
    }
    result(id, {});
    continue;
  }

  if (method === "compaction/summarize") {
    if (!assertCompactionParams(id, params)) continue;
    if (hasMode(Mode.malformedCompactionResult)) {
      result(id, { summary: 42 });
      continue;
    }
    result(id, {
      summary: `conformance compaction summary: ${params.messagesToSummarize.length}`,
      metadata: { conformanceCompaction: true },
    });
    continue;
  }

  if (method === "shutdown") {
    result(id, {});
    process.exit(0);
  }

  error(id, `unknown method: ${method}`, -32601);
}
