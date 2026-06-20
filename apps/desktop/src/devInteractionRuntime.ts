import type {
  AppApprovalResolveRequest,
  AppDisplayEvent,
  AppDisplaySubscribeRequest,
  AppInteractionDisplayNotification,
  AppInteractionEndpoint,
  AppInteractionSessionDescriptor,
  AppMessage,
  AppPromptInput,
  AppPromptRequest,
  AppSessionCreateRequest,
  AppSessionListRequest,
  AppSessionRequest,
  AppSubscriptionResult,
  InteractionInitializeRequest,
  InteractionInitializeResult,
} from "./generated/contracts";
import type {
  InteractionClient,
  InteractionDisplayStream,
  InteractionDisplayStreamHandlers,
} from "./interaction/client";
import {
  DEV_INTERACTION_PROTOCOL_VERSION,
  DEV_INTERACTION_SERVER_NAME,
  DEV_PROFILE_DISPLAY_NAME,
  DEV_PROFILE_ID,
} from "./devFallback";

const DEV_SUBSCRIPTION_ID = "dev-subscription";

type DevInteractionRuntimeObserver = {
  onDisplayEvent?(sessionId: string, event: AppDisplayEvent): void;
};

export function createDevInteractionClient(): InteractionClient {
  return devRuntime.createClient();
}

export function connectDevInteractionDisplayStream(
  endpoint: AppInteractionEndpoint,
  handlers: InteractionDisplayStreamHandlers,
  _request?: Partial<InteractionInitializeRequest>,
): Promise<InteractionDisplayStream> {
  return devRuntime.connectDisplayStream(endpoint, handlers);
}

export function resetDevInteractionRuntimeForTests(): void {
  devRuntime = new DevInteractionRuntime();
}

export function observeDevInteractionRuntimeForTests(
  observer: DevInteractionRuntimeObserver,
): () => void {
  return devRuntime.observe(observer);
}

class DevInteractionRuntime {
  private readonly observers = new Set<DevInteractionRuntimeObserver>();
  private readonly pendingApprovals = new Map<string, string>();
  private readonly sessions = new Map<string, AppInteractionSessionDescriptor>();
  private readonly subscriptions = new Map<string, Set<InteractionDisplayStreamHandlers>>();
  private readonly abortedRuns = new Set<string>();
  private nextSessionNumber = 1;
  private nextMessageNumber = 1;
  private nextRunNumber = 1;

  readonly initializeResult: InteractionInitializeResult = {
    server: { name: DEV_INTERACTION_SERVER_NAME, protocolVersion: DEV_INTERACTION_PROTOCOL_VERSION },
    profiles: [{ profileId: DEV_PROFILE_ID, displayName: DEV_PROFILE_DISPLAY_NAME }],
  };

  constructor() {
    const session = this.createSessionDescriptor("session-1");
    this.sessions.set(session.sessionId, session);
  }

  createClient(): InteractionClient {
    return {
      initialize: async (_request?: Partial<InteractionInitializeRequest>) =>
        structuredClone(this.initializeResult),
      listSessions: async (request?: AppSessionListRequest) =>
        this.listSessions(request?.profileId ?? null),
      createSession: async (request?: AppSessionCreateRequest) => this.createSession(request),
      getSession: async (sessionId: string) => structuredClone(this.requireSession(sessionId)),
    };
  }

  async connectDisplayStream(
    _endpoint: AppInteractionEndpoint,
    handlers: InteractionDisplayStreamHandlers,
  ): Promise<InteractionDisplayStream> {
    return {
      subscribeDisplay: async (
        request: AppDisplaySubscribeRequest,
      ): Promise<AppSubscriptionResult> => {
        const handlersForSession = this.subscriptions.get(request.sessionId) ?? new Set();
        handlersForSession.add(handlers);
        this.subscriptions.set(request.sessionId, handlersForSession);
        return { subscriptionId: DEV_SUBSCRIPTION_ID };
      },
      prompt: async (request: AppPromptRequest) => this.prompt(request),
      abort: async (request: AppSessionRequest) => {
        const session = this.patchSession(request.sessionId, { status: "aborted" });
        const activeRunId = activeRunIdFromSession(session);
        if (activeRunId) {
          this.abortedRuns.add(activeRunId);
        }
        this.expirePendingApproval(request.sessionId, activeRunId);
        this.emit(request.sessionId, {
          type: "run_aborted",
          runId: activeRunId ?? `dev-run-${this.nextRunNumber}`,
        });
        return structuredClone(session);
      },
      resolveApproval: async (request: AppApprovalResolveRequest) =>
        this.resolveApproval(request),
      close: () => {
        for (const handlersForSession of this.subscriptions.values()) {
          handlersForSession.delete(handlers);
        }
      },
    };
  }

  observe(observer: DevInteractionRuntimeObserver): () => void {
    this.observers.add(observer);
    return () => this.observers.delete(observer);
  }

  private listSessions(profileId: string | null): AppInteractionSessionDescriptor[] {
    return [...this.sessions.values()]
      .filter((session) => !profileId || session.profileId === profileId)
      .map((session) => structuredClone(session));
  }

  private createSession(request?: AppSessionCreateRequest): AppInteractionSessionDescriptor {
    this.nextSessionNumber += 1;
    const session = this.createSessionDescriptor(
      request?.sessionId ?? `session-${this.nextSessionNumber}`,
    );
    this.sessions.set(session.sessionId, session);
    return structuredClone(session);
  }

  private async prompt(request: AppPromptRequest): Promise<AppInteractionSessionDescriptor> {
    const session = this.requireSession(request.sessionId);
    const userMessage = inputToMessage(request.input, this.nextMessageId("user"));
    const runNumber = this.nextRunNumber++;
    const runId = `dev-run-${runNumber}`;
    const displayMessageId = `dev-assistant-${this.nextMessageNumber++}`;
    const thoughtId = `dev-thought-${runNumber}`;
    const shouldPauseForApproval = promptText(request.input).toLowerCase().includes("approval");

    this.sessions.set(session.sessionId, {
      ...session,
      status: "running",
      metadata: { ...session.metadata, activeRunId: runId },
      state: {
        messages: [...(session.state.messages ?? []), userMessage],
      },
    });

    this.emit(session.sessionId, { type: "run_started", runId });
    if (shouldPauseForApproval) {
      const approvalId = `approval-${runId}`;
      this.emitApprovalRequest(session.sessionId, runId, approvalId);
      return structuredClone(this.requireSession(session.sessionId));
    }

    this.emit(session.sessionId, { type: "thought_started", runId, thoughtId });
    await delay(120);
    if (this.wasAborted(session.sessionId, runId)) {
      return structuredClone(this.requireSession(session.sessionId));
    }
    this.emit(session.sessionId, {
      type: "thought_delta",
      runId,
      thoughtId,
      kind: "summary",
      text: "Reviewing the UI state and preserving the compact composer rhythm.",
    });
    await delay(120);
    if (this.wasAborted(session.sessionId, runId)) {
      return structuredClone(this.requireSession(session.sessionId));
    }

    this.emit(session.sessionId, {
      type: "tool_started",
      toolCallId: `tool-${runId}`,
      toolName: "desktop.preview.inspect",
    });
    await delay(120);
    if (this.wasAborted(session.sessionId, runId)) {
      return structuredClone(this.requireSession(session.sessionId));
    }
    this.emit(session.sessionId, {
      type: "tool_updated",
      toolCallId: `tool-${runId}`,
      update: { content: [{ type: "text", text: "Captured desktop viewport metrics." }] },
    });
    await delay(120);
    if (this.wasAborted(session.sessionId, runId)) {
      return structuredClone(this.requireSession(session.sessionId));
    }
    this.emit(session.sessionId, {
      type: "tool_completed",
      toolCallId: `tool-${runId}`,
      output: { content: [{ type: "text", text: "Viewport check complete." }] },
    });
    await delay(120);
    if (this.wasAborted(session.sessionId, runId)) {
      return structuredClone(this.requireSession(session.sessionId));
    }
    this.emit(session.sessionId, {
      type: "assistant_message_delta",
      runId,
      displayMessageId,
      text: "This browser preview is running against the dev interaction runtime, so message flow, scroll behavior, and composer alignment can be inspected without a backend.",
    });
    await delay(120);
    if (this.wasAborted(session.sessionId, runId)) {
      return structuredClone(this.requireSession(session.sessionId));
    }

    const assistantMessage = assistantMessageFromText(
      displayMessageId,
      "This browser preview is running against the dev interaction runtime, so message flow, scroll behavior, and composer alignment can be inspected without a backend.",
    );
    const completed = this.patchSession(session.sessionId, {
      status: "completed",
      metadata: { ...this.requireSession(session.sessionId).metadata, activeRunId: null },
      state: {
        messages: [
          ...(this.requireSession(session.sessionId).state.messages ?? []),
          assistantMessage,
        ],
      },
    });
    this.emit(session.sessionId, {
      type: "thought_completed",
      runId,
      thoughtId,
      elapsedMs: 360,
    });
    this.emit(session.sessionId, {
      type: "assistant_message_final",
      runId,
      displayMessageId,
      message: assistantMessage,
    });
    this.emit(session.sessionId, { type: "run_completed", runId });
    return structuredClone(completed);
  }

  private async resolveApproval(
    request: AppApprovalResolveRequest,
  ): Promise<AppInteractionSessionDescriptor> {
    const runId = request.approvalId.replace(/^approval-/, "");
    const session = this.requireSession(request.sessionId);
    const activeRunId = activeRunIdFromSession(session);
    if (session.status === "aborted" || activeRunId !== runId) {
      return structuredClone(session);
    }
    this.abortedRuns.delete(runId);
    this.pendingApprovals.delete(request.sessionId);
    this.emit(request.sessionId, {
      type: "approval_resolved",
      approvalId: request.approvalId,
      decision: request.decision,
    });
    const outcomeText =
      request.decision.outcome === "allow"
        ? "Approval accepted in the dev preview. The flow can continue."
        : "Approval denied in the dev preview. The flow stopped cleanly.";
    const assistantMessage = assistantMessageFromText(
      this.nextMessageId("assistant"),
      outcomeText,
    );
    const completed = this.patchSession(request.sessionId, {
      status: "completed",
      metadata: { ...this.requireSession(request.sessionId).metadata, activeRunId: null },
      state: {
        messages: [
          ...(this.requireSession(request.sessionId).state.messages ?? []),
          assistantMessage,
        ],
      },
    });
    this.emit(request.sessionId, {
      type: "assistant_message_final",
      runId,
      displayMessageId: assistantMessage.id,
      message: assistantMessage,
    });
    this.emit(request.sessionId, { type: "run_completed", runId });
    return structuredClone(completed);
  }

  private createSessionDescriptor(sessionId: string): AppInteractionSessionDescriptor {
    return {
      sessionId,
      profileId: DEV_PROFILE_ID,
      status: "idle",
      state: { messages: [] },
      metadata: {},
    };
  }

  private requireSession(sessionId: string): AppInteractionSessionDescriptor {
    const session = this.sessions.get(sessionId);
    if (!session) {
      throw new Error(`missing dev session: ${sessionId}`);
    }
    return session;
  }

  private patchSession(
    sessionId: string,
    patch: Partial<AppInteractionSessionDescriptor>,
  ): AppInteractionSessionDescriptor {
    const next = {
      ...this.requireSession(sessionId),
      ...patch,
    };
    this.sessions.set(sessionId, next);
    return next;
  }

  private emit(sessionId: string, event: AppDisplayEvent): void {
    const notification: AppInteractionDisplayNotification = {
      sessionId,
      subscriptionId: DEV_SUBSCRIPTION_ID,
      event,
    };
    for (const observer of this.observers) {
      observer.onDisplayEvent?.(sessionId, structuredClone(event));
    }
    for (const handlers of this.subscriptions.get(sessionId) ?? []) {
      handlers.onDisplayEvent(structuredClone(notification));
    }
  }

  private nextMessageId(role: string): string {
    return `dev-${role}-${this.nextMessageNumber++}`;
  }

  private emitApprovalRequest(sessionId: string, runId: string, approvalId: string): void {
    this.pendingApprovals.set(sessionId, approvalId);
    this.emit(sessionId, {
      type: "approval_requested",
      approval: {
        approvalId,
        toolCall: {
          id: `tool-${runId}`,
          name: "desktop.preview.change",
          arguments: {
            command: "apply visual update",
            cwd: "/Users/m4n5ter/rust/noloong",
            targetPaths: ["apps/desktop/src/styles/chat-runtime.css"],
          },
        },
        request: {
          prompt: "Apply this preview-only visual change?",
          reason: "This preview-only action would update local project files.",
        },
        permissions: [
          {
            capability: "write",
            description: "Modify local project files",
          },
        ],
      },
    });
    this.patchSession(sessionId, {
      status: "paused",
      metadata: { ...this.requireSession(sessionId).metadata, activeRunId: runId },
    });
    this.emit(sessionId, {
      type: "run_paused",
      runId,
      reason: { type: "tool_approval", approvalId },
    });
  }

  private wasAborted(sessionId: string, runId: string): boolean {
    if (!this.abortedRuns.delete(runId)) {
      return false;
    }
    this.patchSession(sessionId, {
      status: "aborted",
      metadata: { ...this.requireSession(sessionId).metadata, activeRunId: null },
    });
    return true;
  }

  private expirePendingApproval(sessionId: string, activeRunId: string | null): void {
    const approvalId = this.pendingApprovals.get(sessionId);
    if (!approvalId) {
      return;
    }
    this.pendingApprovals.delete(sessionId);
    this.emit(sessionId, {
      type: "approval_expired",
      approvalId,
      decision: {
        outcome: "deny",
        approver: "noloong-dev-preview",
        reason: "Run was stopped.",
      },
    });
    if (activeRunId) {
      this.abortedRuns.add(activeRunId);
    }
  }
}

let devRuntime = new DevInteractionRuntime();

function inputToMessage(input: AppPromptInput, fallbackId: string): AppMessage {
  if (input.type === "message") {
    return input.message;
  }
  return {
    id: fallbackId,
    role: "user",
    content: [{ type: "text", text: input.text }],
    metadata: {},
  };
}

function promptText(input: AppPromptInput): string {
  if (input.type === "text") {
    return input.text;
  }
  return (input.message.content ?? [])
    .filter((block) => block.type === "text")
    .map((block) => block.text)
    .join("\n");
}

function assistantMessageFromText(id: string, text: string): AppMessage {
  return {
    id,
    role: "assistant",
    content: [{ type: "text", text }],
    metadata: {},
  };
}

function activeRunIdFromSession(session: AppInteractionSessionDescriptor): string | null {
  const value = session.metadata?.activeRunId;
  return typeof value === "string" ? value : null;
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}
