#!/usr/bin/env node

import readline from "node:readline";

const MODEL = "deepseek/deepseek-v4-flash";
const OFFICIAL_PROVIDER = "deepseek";

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

function toChatMessages(messages) {
  return messages
    .map((message) => {
      if (message.role === "tool_result") {
        return undefined;
      }
      const content = (message.content ?? [])
        .filter((block) => block.type === "text")
        .map((block) => block.text)
        .join("\n");
      if (!content) {
        return undefined;
      }
      return {
        role: message.role === "assistant" ? "assistant" : "user",
        content,
      };
    })
    .filter(Boolean);
}

async function callOpenRouter(request) {
  const apiKey = process.env.OPENROUTER_API_KEY;
  if (!apiKey) {
    throw new Error("OPENROUTER_API_KEY is required for this fixture");
  }

  const response = await fetch("https://openrouter.ai/api/v1/chat/completions", {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
      "X-Title": "noloong-agent-core-live-test",
    },
    body: JSON.stringify({
      model: MODEL,
      messages: toChatMessages(request.messages),
      max_tokens: 96,
      temperature: 0,
      reasoning: { enabled: true },
      include_reasoning: true,
      provider: {
        only: [OFFICIAL_PROVIDER],
        allow_fallbacks: false,
        require_parameters: true,
      },
      stream: false,
    }),
  });

  const body = await response.text();
  if (!response.ok) {
    throw new Error(`OpenRouter ${response.status}: ${body}`);
  }
  return JSON.parse(body);
}

for await (const line of rl) {
  if (!line.trim()) continue;
  const request = JSON.parse(line);
  const { id, method, params } = request;

  try {
    if (method === "initialize") {
      result(id, {
        manifest: { name: "openrouter-deepseek-fixture", version: "0.1.0" },
      });
      continue;
    }

    if (method === "capabilities/list") {
      result(id, {
        capabilities: [{ type: "model_provider", id: "openrouter-deepseek-official" }],
      });
      continue;
    }

    if (method === "model/stream") {
      const streamId = params.streamId;
      const completion = await callOpenRouter(params.request);
      const message = completion.choices?.[0]?.message ?? {};
      stream(streamId, { type: "started", stream_id: streamId });
      if (message.reasoning) {
        stream(streamId, { type: "thinking_delta", textDelta: message.reasoning });
      }
      if (message.content) {
        stream(streamId, { type: "text_delta", text: message.content });
      }
      stream(streamId, { type: "finished", stop_reason: "stop" });
      result(id, {
        streamId,
        model: MODEL,
        provider: OFFICIAL_PROVIDER,
        reasoningEnabled: true,
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
  } catch (error) {
    send({
      jsonrpc: "2.0",
      id,
      error: {
        code: -32000,
        message: error instanceof Error ? error.message : String(error),
      },
    });
  }
}
