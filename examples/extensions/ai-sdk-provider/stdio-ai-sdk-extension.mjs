#!/usr/bin/env node

import readline from "node:readline";

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

async function streamWithAiSdk(streamId, request) {
  const [{ streamText }, { openai }] = await Promise.all([
    import("ai"),
    import("@ai-sdk/openai"),
  ]);
  const model = openai(process.env.OPENAI_MODEL ?? "gpt-5.4-mini");
  const prompt = request.messages
    .flatMap((message) =>
      (message.content ?? [])
        .filter((block) => block.type === "text")
        .map((block) => `${message.role}: ${block.text}`),
    )
    .join("\n");

  stream(streamId, { type: "started", stream_id: streamId });
  const response = streamText({ model, prompt });
  for await (const delta of response.textStream) {
    stream(streamId, { type: "text_delta", text: delta });
  }
  stream(streamId, { type: "finished", stop_reason: "stop" });
}

for await (const line of rl) {
  if (!line.trim()) continue;
  const request = JSON.parse(line);
  const { id, method, params } = request;

  if (method === "initialize") {
    result(id, {
      manifest: { name: "noloong-ai-sdk-provider", version: "0.1.0" },
    });
    continue;
  }

  if (method === "capabilities/list") {
    result(id, {
      capabilities: [
        { type: "model_provider", id: "ai-sdk-openai" },
        {
          type: "tool",
          spec: {
            name: "echo",
            description: "Echo tool for validating tool execution over JSON-RPC",
            inputSchema: {
              type: "object",
              properties: { text: { type: "string" } },
              required: ["text"],
            },
          },
        },
        { type: "context_provider", id: "static-context" },
      ],
    });
    continue;
  }

  if (method === "context/apply") {
    result(id, {
      effects: [
        {
          type: "patch_context",
          patch: {
            op: "set",
            key: "ai_sdk_extension",
            value: "enabled",
          },
        },
      ],
    });
    continue;
  }

  if (method === "tool/execute") {
    result(id, {
      content: [{ type: "text", text: params.request.arguments.text ?? "" }],
      details: { source: "ai-sdk-extension" },
      isError: false,
      updates: [],
    });
    continue;
  }

  if (method === "model/stream") {
    const streamId = params.streamId;
    await streamWithAiSdk(streamId, params.request);
    result(id, { streamId });
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
