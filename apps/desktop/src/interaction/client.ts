import type {
  AppApprovalResolveRequest,
  AppDisplaySubscribeRequest,
  AppInteractionDisplayNotification,
  AppInteractionEndpoint,
  AppInteractionSessionDescriptor,
  AppPromptRequest,
  AppSessionCreateRequest,
  AppSessionListRequest,
  AppSessionRequest,
  AppSubscriptionResult,
  InteractionInitializeRequest,
  InteractionInitializeResult,
} from "../generated/contracts";

export type InteractionClient = {
  initialize(request?: Partial<InteractionInitializeRequest>): Promise<InteractionInitializeResult>;
  listSessions(request?: AppSessionListRequest): Promise<AppInteractionSessionDescriptor[]>;
  createSession(request?: AppSessionCreateRequest): Promise<AppInteractionSessionDescriptor>;
  getSession(sessionId: string): Promise<AppInteractionSessionDescriptor>;
};

export type InteractionDisplayStream = {
  subscribeDisplay(request: AppDisplaySubscribeRequest): Promise<AppSubscriptionResult>;
  prompt(request: AppPromptRequest): Promise<AppInteractionSessionDescriptor>;
  abort(request: AppSessionRequest): Promise<AppInteractionSessionDescriptor>;
  resolveApproval(request: AppApprovalResolveRequest): Promise<AppInteractionSessionDescriptor>;
  close(): void;
};

export type InteractionDisplayStreamHandlers = {
  onDisplayEvent(notification: AppInteractionDisplayNotification): void;
  onClose?(): void;
  onError?(error: Error): void;
};

type JsonRpcResponse<T> =
  | { jsonrpc?: "2.0"; id?: number; result: T; error?: never }
  | { jsonrpc?: "2.0"; id?: number; result?: never; error: { code: number; message: string } };

type JsonRpcWebSocketMessage<T = unknown> =
  | { jsonrpc?: "2.0"; id: number; result: T; error?: never }
  | { jsonrpc?: "2.0"; id: number; result?: never; error: { code: number; message: string } }
  | { jsonrpc?: "2.0"; method: string; params: unknown };

const WEBSOCKET_REQUEST_TIMEOUT_MS = 30_000;

export function createInteractionClient(
  endpoint: AppInteractionEndpoint,
  fetchImpl: typeof fetch = fetch,
): InteractionClient {
  const httpUrl = interactionHttpUrl(endpoint.wsUrl);
  const requestId = createRequestId();

  async function call<T>(method: string, params: unknown): Promise<T> {
    const headers: Record<string, string> = {
      "Content-Type": "application/json",
    };
    if (endpoint.bearerToken) {
      headers.Authorization = `Bearer ${endpoint.bearerToken}`;
    }

    const response = await fetchImpl(httpUrl, {
      method: "POST",
      headers,
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: requestId(),
        method,
        params,
      }),
    });

    if (!response.ok) {
      throw new Error(`interaction HTTP ${response.status}: ${response.statusText}`);
    }

    const payload = (await response.json()) as JsonRpcResponse<T>;
    if ("error" in payload && payload.error) {
      throw new Error(`interaction JSON-RPC ${payload.error.code}: ${payload.error.message}`);
    }
    return payload.result;
  }

  return {
    initialize(request) {
      return call("initialize", {
        ...noloongAppInitializeRequest(),
        ...request,
      });
    },
    listSessions(request = {}) {
      return call("session/list", request);
    },
    createSession(request = {}) {
      return call("session/create", request);
    },
    getSession(sessionId) {
      return call("session/get", { sessionId } satisfies AppSessionRequest);
    },
  };
}

export async function connectInteractionDisplayStream(
  endpoint: AppInteractionEndpoint,
  handlers: InteractionDisplayStreamHandlers,
  request?: Partial<InteractionInitializeRequest>,
  WebSocketImpl: typeof WebSocket = WebSocket,
): Promise<InteractionDisplayStream> {
  const stream = new BrowserInteractionDisplayStream(endpoint, handlers, WebSocketImpl);
  await stream.open();
  await stream.initialize(request);
  return stream;
}

export function interactionHttpUrl(wsUrl: string): string {
  const url = new URL(wsUrl);
  if (url.protocol === "ws:") {
    url.protocol = "http:";
  } else if (url.protocol === "wss:") {
    url.protocol = "https:";
  } else {
    throw new Error(`unsupported interaction websocket scheme: ${url.protocol}`);
  }

  if (!url.pathname.endsWith("/ws")) {
    throw new Error(`interaction websocket URL must end with /ws: ${wsUrl}`);
  }

  url.pathname = url.pathname.slice(0, -"/ws".length);
  return url.toString();
}

export function interactionWebSocketUrl(endpoint: AppInteractionEndpoint): string {
  const url = new URL(endpoint.wsUrl);
  if (endpoint.bearerToken) {
    url.searchParams.set("access_token", endpoint.bearerToken);
  }
  return url.toString();
}

export function noloongAppInitializeRequest(): InteractionInitializeRequest {
  return {
    name: "noloong-app",
    requestedAuthority: ["agent.run", "approval.resolve", "session.delete"],
    requestedUx: {
      displayEvents: true,
      streamText: true,
      editMessage: true,
      markdown: true,
    },
    metadata: {},
  };
}

class BrowserInteractionDisplayStream implements InteractionDisplayStream {
  private readonly socket: WebSocket;
  private readonly requestId = createRequestId();
  private readonly pending = new Map<number, PendingWebSocketRequest>();
  private opened = false;
  private closing = false;
  private openPromise: Promise<void>;
  private resolveOpen!: () => void;
  private rejectOpen!: (error: Error) => void;

  constructor(
    endpoint: AppInteractionEndpoint,
    private readonly handlers: InteractionDisplayStreamHandlers,
    WebSocketImpl: typeof WebSocket,
  ) {
    this.openPromise = new Promise((resolve, reject) => {
      this.resolveOpen = resolve;
      this.rejectOpen = reject;
    });
    this.socket = new WebSocketImpl(interactionWebSocketUrl(endpoint));
    this.socket.addEventListener("open", () => {
      this.opened = true;
      this.resolveOpen();
    });
    this.socket.addEventListener("message", (event) => {
      this.handleMessage(event.data);
    });
    this.socket.addEventListener("error", () => {
      const error = new Error("interaction websocket error");
      this.rejectOpen(error);
      this.handlers.onError?.(error);
    });
    this.socket.addEventListener("close", () => {
      const error = new Error("interaction websocket closed");
      if (!this.opened) {
        this.rejectOpen(error);
      }
      this.rejectPending(error);
      if (!this.closing) {
        this.handlers.onClose?.();
      }
    });
  }

  open(): Promise<void> {
    return this.openPromise;
  }

  initialize(request?: Partial<InteractionInitializeRequest>): Promise<InteractionInitializeResult> {
    return this.request("initialize", {
      ...noloongAppInitializeRequest(),
      ...request,
    });
  }

  subscribeDisplay(request: AppDisplaySubscribeRequest): Promise<AppSubscriptionResult> {
    return this.request("display/subscribe", request);
  }

  prompt(request: AppPromptRequest): Promise<AppInteractionSessionDescriptor> {
    return this.request("agent/prompt", request);
  }

  abort(request: AppSessionRequest): Promise<AppInteractionSessionDescriptor> {
    return this.request("agent/abort", request);
  }

  resolveApproval(request: AppApprovalResolveRequest): Promise<AppInteractionSessionDescriptor> {
    return this.request("approval/resolve", request);
  }

  close(): void {
    this.closing = true;
    this.rejectPending(new Error("interaction websocket closed"));
    this.socket.close();
  }

  private request<T>(method: string, params: unknown): Promise<T> {
    if (this.socket.readyState !== undefined && this.socket.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error("interaction websocket is not open"));
    }
    const id = this.requestId();
    const payload = JSON.stringify({
      jsonrpc: "2.0",
      id,
      method,
      params,
    });
    const promise = new Promise<T>((resolve, reject) => {
      const timeout = globalThis.setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`interaction websocket request timed out: ${method}`));
      }, WEBSOCKET_REQUEST_TIMEOUT_MS);
      this.pending.set(id, {
        resolve: (value) => {
          globalThis.clearTimeout(timeout);
          resolve(value as T);
        },
        reject: (error) => {
          globalThis.clearTimeout(timeout);
          reject(error);
        },
      });
    });
    try {
      this.socket.send(payload);
    } catch (error) {
      const pending = this.pending.get(id);
      this.pending.delete(id);
      pending?.reject(error instanceof Error ? error : new Error(String(error)));
    }
    return promise;
  }

  private handleMessage(data: unknown): void {
    if (typeof data !== "string") {
      this.handlers.onError?.(new Error("interaction websocket sent a non-text message"));
      return;
    }

    let message: JsonRpcWebSocketMessage;
    try {
      message = JSON.parse(data) as JsonRpcWebSocketMessage;
    } catch (error) {
      this.handlers.onError?.(new Error(`invalid interaction websocket JSON: ${String(error)}`));
      return;
    }

    if ("method" in message) {
      if (message.method === "display/event") {
        this.handlers.onDisplayEvent(message.params as AppInteractionDisplayNotification);
      }
      return;
    }

    const pending = this.pending.get(message.id);
    if (!pending) {
      return;
    }
    this.pending.delete(message.id);

    if ("error" in message && message.error) {
      pending.reject(
        new Error(`interaction JSON-RPC ${message.error.code}: ${message.error.message}`),
      );
      return;
    }
    pending.resolve(message.result);
  }

  private rejectPending(error: Error): void {
    for (const pending of this.pending.values()) {
      pending.reject(error);
    }
    this.pending.clear();
  }
}

type PendingWebSocketRequest = {
  resolve(value: unknown): void;
  reject(error: Error): void;
};

function createRequestId(): () => number {
  let nextId = 0;
  return () => {
    nextId += 1;
    return nextId;
  };
}
