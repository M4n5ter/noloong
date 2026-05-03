#!/usr/bin/env node

import readline from "node:readline";

if (process.argv.includes("--invalid-json")) {
  process.stdout.write("{not-json}\n");
  setInterval(() => {}, 1000);
}
const delayedStream = process.argv.includes("--delayed-stream");
const streamError = process.argv.includes("--stream-error");
const streamHangs = process.argv.includes("--stream-hangs");
const streamNoResponse = process.argv.includes("--stream-no-response");
const crashOnModel = process.argv.includes("--crash-on-model");
const requestTimeoutOnModel = process.argv.includes("--request-timeout-on-model");
const mediaStream = process.argv.includes("--media-stream");
const mediaTool = process.argv.includes("--media-tool");
const phaseHookMode =
  process.argv
    .find((arg) => arg.startsWith("--phase-hook-mode="))
    ?.slice("--phase-hook-mode=".length) ?? null;
const toolHookMode =
  process.argv
    .find((arg) => arg.startsWith("--tool-hook-mode="))
    ?.slice("--tool-hook-mode=".length) ?? null;
const compactionSummarizerMode =
  process.argv
    .find((arg) => arg.startsWith("--compaction-summarizer-mode="))
    ?.slice("--compaction-summarizer-mode=".length) ?? null;

const rl = readline.createInterface({
  input: process.stdin,
  crlfDelay: Infinity,
});

function send(message) {
  process.stdout.write(`${JSON.stringify(message)}\n`);
}

function result(id, value) {
  send({ jsonrpc: "2.0", id, result: value });
}

function stream(streamId, event) {
  send({
    jsonrpc: "2.0",
    method: "stream/event",
    params: { streamId, event },
  });
}

for await (const line of rl) {
  if (!line.trim()) continue;
  const request = JSON.parse(line);
  const { id, method, params } = request;

  if (method === "initialize") {
    result(id, {
      manifest: { name: "stdio-fixture", version: "0.1.0" },
    });
    continue;
  }

  if (method === "capabilities/list") {
    const capabilities = [
      { type: "model_provider", id: "fixture-model" },
      {
        type: "tool",
        spec: {
          name: "fixture_echo",
          description: "Echo text from a stdio JSON-RPC fixture",
          inputSchema: {
            type: "object",
            properties: { text: { type: "string" } },
            required: ["text"],
          },
          permissions: [
            {
              capability: "fixture.echo",
              description: "Allows fixture echo execution",
              metadata: { fixture: "permission" },
            },
          ],
        },
      },
      { type: "context_provider", id: "fixture-context" },
      { type: "phase_node", id: "fixture.phase" },
    ];
    if (phaseHookMode) {
      capabilities.push({ type: "phase_hook", id: "fixture-phase-hook" });
    }
    if (toolHookMode) {
      capabilities.push({ type: "tool_call_hook", id: "fixture-tool-hook" });
    }
    if (compactionSummarizerMode) {
      capabilities.push({
        type: "compaction_summarizer",
        id: "fixture-compaction",
      });
    }
    result(id, {
      capabilities,
    });
    continue;
  }

  if (method === "context/apply") {
    result(id, {
      effects: [
        {
          type: "patch_context",
          patch: { op: "set", key: "fixture", value: "context" },
        },
      ],
    });
    continue;
  }

  if (method === "model/stream") {
    if (crashOnModel) {
      process.exit(42);
    }
    if (requestTimeoutOnModel) {
      continue;
    }
    const streamId = params.streamId;
    const messages = params.request.messages ?? [];
    const hasToolResult = messages.some((message) => message.role === "tool_result");
    stream(streamId, { type: "started", stream_id: streamId });
    if (params.request.metadata?.fixtureHook === "before_model_request") {
      stream(streamId, { type: "text_delta", text: "hooked request" });
      stream(streamId, { type: "finished", stop_reason: "stop" });
      result(id, { streamId });
      continue;
    }
    if (streamError) {
      stream(streamId, { type: "failed", error: "fixture stream failed" });
      result(id, { streamId });
      continue;
    }
    if (streamHangs) {
      stream(streamId, { type: "text_delta", text: "hanging chunk" });
      continue;
    }
    if (streamNoResponse) {
      stream(streamId, { type: "text_delta", text: "terminal chunk" });
      stream(streamId, { type: "finished", stop_reason: "stop" });
      continue;
    }
    if (mediaStream) {
      stream(streamId, {
        type: "media_delta",
        kind: "image",
        dataDelta: "aW1hZ2U=",
        mimeType: "image/png",
        done: true,
      });
      stream(streamId, { type: "finished", stop_reason: "stop" });
      result(id, { streamId });
      continue;
    }
    if (delayedStream) {
      stream(streamId, { type: "text_delta", text: "delayed chunk" });
      await new Promise((resolve) => setTimeout(resolve, 150));
      stream(streamId, { type: "finished", stop_reason: "stop" });
      result(id, { streamId });
      continue;
    }
    if (hasToolResult) {
      stream(streamId, { type: "text_delta", text: "done from fixture" });
      stream(streamId, { type: "finished", stop_reason: "stop" });
    } else {
      stream(streamId, {
        type: "tool_call",
        tool_call: {
          id: "fixture-call-1",
          name: "fixture_echo",
          arguments: { text: "from fixture model" },
        },
      });
      stream(streamId, { type: "finished", stop_reason: "tool_use" });
    }
    result(id, { streamId });
    continue;
  }

  if (method === "phase/run") {
    result(id, {
      scratch: params.request.scratch,
      effects: [
        {
          type: "patch_context",
          patch: { op: "set", key: "fixture_phase", value: true },
        },
      ],
      streamEvents: [],
      resolvedToolCalls: [],
      toolOutputs: [],
    });
    continue;
  }

  if (method === "phase_hook/run") {
    if (phaseHookMode === "malformed") {
      result(id, { modelRequest: "not an object" });
      continue;
    }
    if (params.hookPoint === "before_model_request" && phaseHookMode === "before-request") {
      const modelRequest = params.modelRequest;
      modelRequest.metadata = {
        ...(modelRequest.metadata ?? {}),
        fixtureHook: "before_model_request",
      };
      result(id, { modelRequest });
      continue;
    }
    if (params.hookPoint === "after_model_request" && phaseHookMode === "after-events") {
      result(id, {
        modelEvents: [
          { type: "started", stream_id: "phase-hook-stream" },
          { type: "text_delta", text: "hooked events" },
          { type: "finished", stop_reason: "stop" },
        ],
      });
      continue;
    }
    if (params.hookPoint === "after_assistant_commit" && phaseHookMode === "after-assistant") {
      const assistantMessage = params.assistantMessage;
      assistantMessage.content = [{ type: "text", text: "hooked assistant" }];
      result(id, { assistantMessage });
      continue;
    }
    result(id, {});
    continue;
  }

  if (method === "tool_hook/run") {
    if (toolHookMode === "malformed") {
      result(id, { decision: "not an object" });
      continue;
    }
    if (params.hookPoint === "before_tool_call" && toolHookMode === "deny") {
      result(id, {
        decision: {
          outcome: "deny",
          reason: "denied by fixture tool hook",
          approver: "stdio-fixture",
          metadata: { fixtureHook: "deny" },
        },
      });
      continue;
    }
    if (params.hookPoint === "before_tool_call" && toolHookMode === "allow") {
      result(id, {
        decision: {
          outcome: "allow",
          reason: "allowed by fixture tool hook",
          approver: "stdio-fixture",
          metadata: { fixtureHook: "allow" },
        },
      });
      continue;
    }
    result(id, {});
    continue;
  }

  if (method === "compaction/summarize") {
    if (compactionSummarizerMode === "malformed") {
      result(id, { summary: 42 });
      continue;
    }
    result(id, {
      summary: `fixture compaction summary: ${params.messagesToSummarize.length}`,
      metadata: { fixtureCompaction: true },
    });
    continue;
  }

  if (method === "tool/execute") {
    if (mediaTool) {
      result(id, {
        content: [
          {
            type: "media",
            media: {
              kind: "file",
              source: {
                type: "provider",
                providerId: "fixture-model",
                id: "fixture-file-1",
              },
              mimeType: "application/pdf",
              name: "fixture.pdf",
            },
          },
        ],
        details: { source: "stdio-fixture" },
        isError: false,
        updates: [],
      });
      continue;
    }
    result(id, {
      content: [{ type: "text", text: params.request.arguments.text }],
      details: { source: "stdio-fixture" },
      isError: false,
      updates: [
        {
          content: [{ type: "text", text: "fixture running" }],
          details: { step: 1 },
        },
      ],
    });
    continue;
  }

  if (method === "shutdown") {
    result(id, {});
    process.exit(0);
  }

  send({
    jsonrpc: "2.0",
    id,
    error: { code: -32601, message: `unknown method: ${method}` },
  });
}
