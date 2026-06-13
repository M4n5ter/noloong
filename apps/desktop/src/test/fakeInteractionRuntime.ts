import type {
  AppApprovalResolveRequest,
  AppDisplayEvent,
  AppDisplaySubscribeRequest,
  AppInteractionDisplayNotification,
  AppInteractionEndpoint,
  AppInteractionSessionDescriptor,
  AppPromptRequest,
  AppSessionCreateRequest,
  AppSessionRequest,
  AppSubscriptionResult,
  InteractionInitializeRequest,
  InteractionInitializeResult,
} from "../generated/contracts";
import type {
  InteractionClient,
  InteractionDisplayStream,
  InteractionDisplayStreamHandlers,
} from "../interaction/client";

export class FakeInteractionRuntime {
  readonly endpoint: AppInteractionEndpoint = {
    wsUrl: "ws://127.0.0.1:7777/jsonrpc/ws",
  };
  readonly initializeResult: InteractionInitializeResult = {
    server: { name: "fake-interaction", protocolVersion: "test" },
    profiles: [{ profileId: "default", displayName: "Default" }],
  };
  promptRequests: AppPromptRequest[] = [];
  approvalResolveRequests: AppApprovalResolveRequest[] = [];

  private readonly sessions = new Map<string, AppInteractionSessionDescriptor>();
  private displayHandlers: InteractionDisplayStreamHandlers | null = null;
  private createSessionFailure: Error | null = null;
  private promptDeferred: Deferred<AppInteractionSessionDescriptor> | null = null;
  private queuedGetSessionResponses: AppInteractionSessionDescriptor[] = [];

  constructor(session: AppInteractionSessionDescriptor = emptySession()) {
    this.sessions.set(session.sessionId, clone(session));
  }

  bootstrap(locale: "en" | "zh" = "en") {
    return {
      appVersion: "test",
      interactionEndpoint: this.endpoint,
      interactionStatus: {
        status: "ready" as const,
        serverName: "fake-interaction",
        protocolVersion: "test",
        profiles: [{ profileId: "default", displayName: "Default" }],
      },
      locale,
      profileConfigPath: "/tmp/test-profile.jsonc",
    };
  }

  createClient = (): InteractionClient => ({
    initialize: async (_request?: Partial<InteractionInitializeRequest>) => this.initializeResult,
    listSessions: async () => [...this.sessions.values()].map(clone),
    createSession: async (_request?: AppSessionCreateRequest) => {
      if (this.createSessionFailure) {
        const error = this.createSessionFailure;
        this.createSessionFailure = null;
        throw error;
      }
      const session = emptySession(`session-${this.sessions.size + 1}`);
      this.sessions.set(session.sessionId, session);
      return clone(session);
    },
    getSession: async (sessionId: string) => {
      const queued = this.queuedGetSessionResponses.shift();
      return clone(queued ?? this.requireSession(sessionId));
    },
  });

  connectDisplayStream = async (
    _endpoint: AppInteractionEndpoint,
    handlers: InteractionDisplayStreamHandlers,
  ): Promise<InteractionDisplayStream> => {
    this.displayHandlers = handlers;
    return {
      subscribeDisplay: async (_request: AppDisplaySubscribeRequest): Promise<AppSubscriptionResult> => ({
        subscriptionId: "subscription-1",
      }),
      prompt: async (request: AppPromptRequest): Promise<AppInteractionSessionDescriptor> => {
        this.promptRequests.push(request);
        this.promptDeferred = createDeferred<AppInteractionSessionDescriptor>();
        return this.promptDeferred.promise;
      },
      abort: async (request: AppSessionRequest) => {
        const session = {
          ...this.requireSession(request.sessionId),
          status: "aborted" as const,
        };
        this.sessions.set(session.sessionId, session);
        return clone(session);
      },
      resolveApproval: async (request: AppApprovalResolveRequest) => {
        this.approvalResolveRequests.push(request);
        return clone(this.requireSession(request.sessionId));
      },
      close: () => {
        this.displayHandlers = null;
      },
    };
  };

  setSession(session: AppInteractionSessionDescriptor): void {
    this.sessions.set(session.sessionId, clone(session));
  }

  queueGetSessionResponse(session: AppInteractionSessionDescriptor): void {
    this.queuedGetSessionResponses.push(clone(session));
  }

  failNextCreateSession(message = "create session failed"): void {
    this.createSessionFailure = new Error(message);
  }

  emitDisplayEvent(event: AppDisplayEvent, sessionId = "session-1"): void {
    this.displayHandlers?.onDisplayEvent({
      sessionId,
      subscriptionId: "subscription-1",
      event,
    } satisfies AppInteractionDisplayNotification);
  }

  emitAssistantDelta(text: string, displayMessageId = "display-1"): void {
    this.emitDisplayEvent({
      type: "assistant_message_delta",
      runId: "run-1",
      displayMessageId,
      text,
    });
  }

  emitRunCompleted(): void {
    this.emitDisplayEvent({
      type: "run_completed",
      runId: "run-1",
    });
  }

  resolvePrompt(session: AppInteractionSessionDescriptor = this.requireSession("session-1")): void {
    this.promptDeferred?.resolve(clone(session));
    this.promptDeferred = null;
  }

  private requireSession(sessionId: string): AppInteractionSessionDescriptor {
    const session = this.sessions.get(sessionId);
    if (!session) {
      throw new Error(`missing fake session: ${sessionId}`);
    }
    return session;
  }
}

export function emptySession(sessionId = "session-1"): AppInteractionSessionDescriptor {
  return {
    sessionId,
    profileId: "default",
    status: "idle",
    state: { messages: [] },
    metadata: {},
  };
}

export function completedSessionWithText(text: string): AppInteractionSessionDescriptor {
  return {
    sessionId: "session-1",
    profileId: "default",
    status: "completed",
    state: {
      messages: [
        {
          id: "assistant-final",
          role: "assistant",
          content: [{ type: "text", text }],
        },
      ],
    },
    metadata: {},
  };
}

type Deferred<T> = {
  promise: Promise<T>;
  resolve(value: T): void;
};

function createDeferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

function clone<T>(value: T): T {
  return structuredClone(value);
}
