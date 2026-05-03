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
    result(id, {
      capabilities: [
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
          },
        },
        { type: "context_provider", id: "fixture-context" },
        { type: "phase_node", id: "fixture.phase" },
      ],
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
