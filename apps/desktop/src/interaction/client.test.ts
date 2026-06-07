import { describe, expect, it, vi } from "vitest";
import {
  connectInteractionDisplayStream,
  createInteractionClient,
  interactionHttpUrl,
  interactionWebSocketUrl,
} from "./client";

describe("interactionHttpUrl", () => {
  it("derives HTTP JSON-RPC endpoint from websocket endpoint", () => {
    expect(interactionHttpUrl("ws://127.0.0.1:8787/jsonrpc/ws")).toBe(
      "http://127.0.0.1:8787/jsonrpc",
    );
    expect(interactionHttpUrl("wss://noloong.example/jsonrpc/ws")).toBe(
      "https://noloong.example/jsonrpc",
    );
  });

  it("rejects websocket URLs without the /ws suffix", () => {
    expect(() => interactionHttpUrl("ws://127.0.0.1:8787/jsonrpc")).toThrow(
      "must end with /ws",
    );
  });
});

describe("interactionWebSocketUrl", () => {
  it("passes bearer tokens through a browser-compatible query parameter", () => {
    expect(
      interactionWebSocketUrl({
        wsUrl: "ws://127.0.0.1:8787/jsonrpc/ws",
        bearerToken: "token",
      }),
    ).toBe("ws://127.0.0.1:8787/jsonrpc/ws?access_token=token");
  });
});

describe("createInteractionClient", () => {
  it("sends typed JSON-RPC requests directly to the interaction HTTP endpoint", async () => {
    const fetchImpl = vi.fn(async (_url: string | URL | Request, init?: RequestInit) => {
      const body = JSON.parse(String(init?.body)) as { method: string };
      return jsonResponse({
        jsonrpc: "2.0",
        id: 1,
        result: resultFor(body.method),
      });
    });

    const client = createInteractionClient(
      {
        wsUrl: "ws://127.0.0.1:8787/jsonrpc/ws",
        bearerToken: "token",
      },
      fetchImpl as unknown as typeof fetch,
    );

    await expect(client.initialize()).resolves.toEqual({
      server: { name: "noloong-agent", protocolVersion: "2026-05-05" },
      profiles: [],
    });
    await expect(client.listSessions({ profileId: "default" })).resolves.toEqual([]);
    await expect(client.createSession({ profileId: "default" })).resolves.toMatchObject({
      sessionId: "session-1",
    });
    await expect(client.getSession("session-1")).resolves.toMatchObject({
      sessionId: "session-1",
    });

    const calls = fetchImpl.mock.calls.map(([url, init]) => ({
      url,
      headers: init?.headers,
      body: JSON.parse(String(init?.body)) as { method: string; params: unknown },
    }));

    expect(calls.map((call) => call.body.method)).toEqual([
      "initialize",
      "session/list",
      "session/create",
      "session/get",
    ]);
    expect(calls[1].body.params).toEqual({ profileId: "default" });
    expect(calls[0].url).toBe("http://127.0.0.1:8787/jsonrpc");
    expect(calls[0].headers).toMatchObject({
      Authorization: "Bearer token",
      "Content-Type": "application/json",
    });
  });

  it("reports JSON-RPC errors", async () => {
    const client = createInteractionClient(
      { wsUrl: "ws://127.0.0.1:8787/jsonrpc/ws" },
      (async () =>
        jsonResponse({
          jsonrpc: "2.0",
          id: 1,
          error: { code: -32603, message: "store error" },
        })) as typeof fetch,
    );

    await expect(client.listSessions()).rejects.toThrow(
      "interaction JSON-RPC -32603: store error",
    );
  });
});

describe("connectInteractionDisplayStream", () => {
  it("uses one websocket connection for initialize, display subscription, prompt, and notifications", async () => {
    const events: unknown[] = [];
    const stream = await connectInteractionDisplayStream(
      {
        wsUrl: "ws://127.0.0.1:8787/jsonrpc/ws",
        bearerToken: "token",
      },
      {
        onDisplayEvent(notification) {
          events.push(notification.event);
        },
      },
      { version: "test-version" },
      FakeWebSocket as unknown as typeof WebSocket,
    );
    const socket = FakeWebSocket.last();

    await expect(
      stream.subscribeDisplay({
        sessionId: "session-1",
        ux: { displayEvents: true, streamText: true },
      }),
    ).resolves.toEqual({ subscriptionId: "subscription-1" });
    await expect(
      stream.prompt({
        sessionId: "session-1",
        input: { type: "text", text: "hello" },
      }),
    ).resolves.toMatchObject({ sessionId: "session-1" });
    await expect(stream.abort({ sessionId: "session-1" })).resolves.toMatchObject({
      sessionId: "session-1",
    });
    await expect(
      stream.resolveApproval({
        sessionId: "session-1",
        approvalId: "approval-1",
        decision: { outcome: "allow" },
      }),
    ).resolves.toMatchObject({ sessionId: "session-1" });

    socket.emitNotification({
      sessionId: "session-1",
      subscriptionId: "subscription-1",
      event: {
        type: "assistant_message_delta",
        runId: "run-1",
        displayMessageId: "display-1",
        text: "chunk",
      },
    });

    expect(socket.url).toBe("ws://127.0.0.1:8787/jsonrpc/ws?access_token=token");
    expect(socket.sentMethods()).toEqual([
      "initialize",
      "display/subscribe",
      "agent/prompt",
      "agent/abort",
      "approval/resolve",
    ]);
    expect(events).toEqual([
      {
        type: "assistant_message_delta",
        runId: "run-1",
        displayMessageId: "display-1",
        text: "chunk",
      },
    ]);

    stream.close();
  });
});

function resultFor(method: string): unknown {
  if (method === "initialize") {
    return {
      server: { name: "noloong-agent", protocolVersion: "2026-05-05" },
      profiles: [],
    };
  }
  if (method === "session/list") {
    return [];
  }
  return {
    sessionId: "session-1",
    profileId: "default",
    status: "idle",
    state: { messages: [] },
    metadata: {},
  };
}

function jsonResponse(payload: unknown): Response {
  return new Response(JSON.stringify(payload), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

type FakeWebSocketListener = (event: { data?: string }) => void;

class FakeWebSocket {
  static readonly instances: FakeWebSocket[] = [];
  readonly sent: string[] = [];
  private readonly listeners = new Map<string, FakeWebSocketListener[]>();

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this);
    queueMicrotask(() => {
      this.emit("open", {});
    });
  }

  static last(): FakeWebSocket {
    const socket = FakeWebSocket.instances.at(-1);
    if (!socket) {
      throw new Error("fake websocket was not created");
    }
    return socket;
  }

  addEventListener(type: string, listener: FakeWebSocketListener): void {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }

  send(data: string): void {
    this.sent.push(data);
    const request = JSON.parse(data) as { id: number; method: string; params: unknown };
    queueMicrotask(() => {
      this.emit("message", {
        data: JSON.stringify({
          jsonrpc: "2.0",
          id: request.id,
          result: fakeWebSocketResult(request.method),
        }),
      });
    });
  }

  close(): void {
    this.emit("close", {});
  }

  sentMethods(): string[] {
    return this.sent.map((payload) => (JSON.parse(payload) as { method: string }).method);
  }

  emitNotification(params: unknown): void {
    this.emit("message", {
      data: JSON.stringify({
        jsonrpc: "2.0",
        method: "display/event",
        params,
      }),
    });
  }

  private emit(type: string, event: { data?: string }): void {
    for (const listener of this.listeners.get(type) ?? []) {
      listener(event);
    }
  }
}

function fakeWebSocketResult(method: string): unknown {
  if (method === "initialize") {
    return {
      server: { name: "noloong-agent", protocolVersion: "2026-05-05" },
      profiles: [],
    };
  }
  if (method === "display/subscribe") {
    return { subscriptionId: "subscription-1" };
  }
  if (method === "agent/prompt" || method === "agent/abort" || method === "approval/resolve") {
    return {
      sessionId: "session-1",
      profileId: "default",
      status: "completed",
      state: { messages: [] },
      metadata: {},
    };
  }
  throw new Error(`unexpected fake websocket method: ${method}`);
}
