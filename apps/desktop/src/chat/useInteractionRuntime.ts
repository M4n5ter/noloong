import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { AppToolPermissionOutcome, AppLaunchOptions } from "../generated/contracts";
import {
  appendLocalPrompt,
  applyDisplayEventToConversation,
  conversationFromSessionSnapshot,
  convergeConversationToSessionSnapshot,
  markApprovalResolved,
  setReasoningExpanded,
} from "../interaction/conversationState";
import {
  connectInteractionDisplayStream as connectDefaultInteractionDisplayStream,
  createInteractionClient as createDefaultInteractionClient,
  type InteractionDisplayStream,
} from "../interaction/client";
import type { AppI18n } from "../i18n";
import {
  listSessionsForProfiles,
  sessionStatusFromDisplayEvent,
  updateSessionStatus,
} from "./sessionHelpers";
import { optimisticPromptText, submissionContentBlocks, type PromptSubmission } from "./attachments";
import type { InteractionReadyState, InteractionState } from "./types";

type UseInteractionRuntimeOptions = {
  connectDisplayStream: typeof connectDefaultInteractionDisplayStream;
  createInteractionClient: typeof createDefaultInteractionClient;
  i18n: AppI18n;
  options: AppLaunchOptions;
};

export type InteractionRuntime = {
  interaction: InteractionState;
  resolvingApprovalIds: ReadonlySet<string>;
  createSession(): Promise<string | null>;
  selectSession(sessionId: string): Promise<void>;
  submitPrompt(submission: PromptSubmission): Promise<void>;
  abortCurrentRun(): Promise<void>;
  resolveApproval(approvalId: string, outcome: AppToolPermissionOutcome): Promise<void>;
  toggleReasoning(thoughtId: string, expanded: boolean): void;
};

export function useInteractionRuntime({
  connectDisplayStream,
  createInteractionClient,
  i18n,
  options,
}: UseInteractionRuntimeOptions): InteractionRuntime {
  const client = useMemo(() => {
    return options.interactionEndpoint
      ? createInteractionClient(options.interactionEndpoint)
      : null;
  }, [createInteractionClient, options.interactionEndpoint]);
  const [interaction, setInteraction] = useState<InteractionState>({ status: "loading" });
  const readyRef = useRef<InteractionReadyState | null>(null);
  const streamRef = useRef<InteractionDisplayStream | null>(null);
  const selectedSessionIdRef = useRef<string | null>(null);
  const subscriptionPromisesRef = useRef(new Map<string, Promise<void>>());
  const terminalRefreshTimersRef = useRef(new Map<string, number[]>());
  const resolvingApprovalIdsRef = useRef(new Set<string>());
  const [resolvingApprovalIds, setResolvingApprovalIds] = useState<ReadonlySet<string>>(
    new Set(),
  );

  useEffect(() => {
    const ready = interaction.status === "ready" ? interaction : null;
    readyRef.current = ready;
    selectedSessionIdRef.current = ready?.selectedSessionId ?? null;
  }, [interaction]);

  const clearTerminalRefreshTimers = useCallback((sessionId?: string) => {
    const entries = sessionId
      ? [[sessionId, terminalRefreshTimersRef.current.get(sessionId) ?? []] as const]
      : [...terminalRefreshTimersRef.current.entries()];
    for (const [key, timers] of entries) {
      for (const timer of timers) {
        window.clearTimeout(timer);
      }
      terminalRefreshTimersRef.current.delete(key);
    }
  }, []);

  const refreshSessionSnapshot = useCallback(
    async (sessionId: string) => {
      if (!client) {
        return;
      }
      const selectedSession = await client.getSession(sessionId);
      setInteraction((current) =>
        current.status === "ready" && current.selectedSessionId === sessionId
          ? {
              ...current,
              selectedSession,
              conversation: convergeConversationToSessionSnapshot(
                current.conversation,
                selectedSession,
              ),
              refreshing: false,
              sending: false,
            }
          : current,
      );
    },
    [client],
  );

  const scheduleTerminalSnapshotRefresh = useCallback(
    (sessionId: string) => {
      clearTerminalRefreshTimers(sessionId);
      const timers: number[] = [];
      for (const delayMs of [0, 250, 1000, 2500]) {
        const timer = window.setTimeout(() => {
          const remaining = terminalRefreshTimersRef.current
            .get(sessionId)
            ?.filter((item) => item !== timer);
          if (remaining && remaining.length > 0) {
            terminalRefreshTimersRef.current.set(sessionId, remaining);
          } else {
            terminalRefreshTimersRef.current.delete(sessionId);
          }
          void refreshSessionSnapshot(sessionId).catch((error: unknown) => {
            setInteraction((current) =>
              current.status === "ready"
                ? { ...current, streamError: String(error), streamStatus: "failed" }
                : current,
            );
          });
        }, delayMs);
        timers.push(timer);
      }
      terminalRefreshTimersRef.current.set(sessionId, timers);
    },
    [clearTerminalRefreshTimers, refreshSessionSnapshot],
  );

  const load = useCallback(async () => {
    if (!client) {
      setInteraction({
        status: "disconnected",
        launchStatus: options.interactionStatus ?? null,
      });
      return;
    }

    setInteraction({ status: "loading" });
    try {
      const initializeResult = await client.initialize({
        version: options.appVersion,
      });
      const sessions = await listSessionsForProfiles(client, initializeResult);
      const selectedSession = sessions[0]
        ? await client.getSession(sessions[0].sessionId)
        : null;
      setInteraction({
        status: "ready",
        initializeResult,
        sessions,
        selectedSessionId: selectedSession?.sessionId ?? null,
        selectedSession,
        conversation: conversationFromSessionSnapshot(selectedSession),
        refreshing: false,
        sending: false,
        streamStatus: options.interactionEndpoint
          ? streamRef.current
            ? "ready"
            : "connecting"
          : "failed",
        streamError: options.interactionEndpoint ? null : i18n.t("runtime.disconnected"),
      });
    } catch (error) {
      setInteraction({ status: "failed", error: String(error) });
    }
  }, [client, i18n, options.appVersion, options.interactionEndpoint, options.interactionStatus]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (!options.interactionEndpoint) {
      return;
    }

    let active = true;
    subscriptionPromisesRef.current.clear();
    setInteraction((current) =>
      current.status === "ready"
        ? { ...current, streamStatus: "connecting", streamError: null }
        : current,
    );

    void connectDisplayStream(
      options.interactionEndpoint,
      {
        onDisplayEvent(notification) {
          const nextStatus = sessionStatusFromDisplayEvent(notification.event);
          const selected = notification.sessionId === selectedSessionIdRef.current;

          setInteraction((current) => {
            if (current.status !== "ready") {
              return current;
            }
            return {
              ...current,
              sessions: nextStatus
                ? updateSessionStatus(current.sessions, notification.sessionId, nextStatus)
                : current.sessions,
              selectedSession:
                selected && nextStatus && current.selectedSession
                  ? { ...current.selectedSession, status: nextStatus }
                  : current.selectedSession,
              conversation: selected
                ? applyDisplayEventToConversation(current.conversation, notification.event)
                : current.conversation,
              sending: selected
                ? current.sending &&
                  !["run_completed", "run_failed", "run_aborted"].includes(
                    notification.event.type,
                  )
                : current.sending,
            };
          });

          if (
            selected &&
            (notification.event.type === "run_completed" ||
              notification.event.type === "run_failed" ||
              notification.event.type === "run_aborted")
          ) {
            scheduleTerminalSnapshotRefresh(notification.sessionId);
          }
        },
        onClose() {
          if (!active) {
            return;
          }
          setInteraction((current) =>
            current.status === "ready"
              ? {
                  ...current,
                  streamStatus: "failed",
                  streamError: i18n.t("runtime.interrupted"),
                }
              : current,
          );
        },
        onError(error) {
          setInteraction((current) =>
            current.status === "ready"
              ? {
                  ...current,
                  streamStatus: "failed",
                  streamError: error.message,
                }
              : current,
          );
        },
      },
      { version: options.appVersion },
    )
      .then((stream) => {
        if (!active) {
          stream.close();
          return;
        }
        streamRef.current = stream;
        setInteraction((current) =>
          current.status === "ready"
            ? { ...current, streamStatus: "ready", streamError: null }
            : current,
        );
      })
      .catch((error: unknown) => {
        setInteraction((current) =>
          current.status === "ready"
            ? { ...current, streamStatus: "failed", streamError: String(error) }
            : current,
        );
      });

    return () => {
      active = false;
      streamRef.current?.close();
      streamRef.current = null;
      subscriptionPromisesRef.current.clear();
      clearTerminalRefreshTimers();
    };
  }, [
    clearTerminalRefreshTimers,
    connectDisplayStream,
    i18n,
    options.appVersion,
    options.interactionEndpoint,
    scheduleTerminalSnapshotRefresh,
  ]);

  const ensureDisplaySubscription = useCallback(
    (sessionId: string): Promise<void> => {
      const existing = subscriptionPromisesRef.current.get(sessionId);
      if (existing) {
        return existing;
      }
      const stream = streamRef.current;
      if (!stream) {
        return Promise.reject(new Error(i18n.t("runtime.disconnected")));
      }
      const promise = stream
        .subscribeDisplay({
          sessionId,
          ux: {
            displayEvents: true,
            streamText: true,
            editMessage: true,
            markdown: true,
          },
        })
        .then(() => undefined)
        .catch((error: unknown) => {
          subscriptionPromisesRef.current.delete(sessionId);
          throw error;
        });
      subscriptionPromisesRef.current.set(sessionId, promise);
      return promise;
    },
    [i18n],
  );

  const selectedSessionId =
    interaction.status === "ready" ? interaction.selectedSessionId : null;
  const streamStatus = interaction.status === "ready" ? interaction.streamStatus : null;

  useEffect(() => {
    if (!selectedSessionId || streamStatus !== "ready") {
      return;
    }
    void ensureDisplaySubscription(selectedSessionId).catch((error: unknown) => {
      setInteraction((current) =>
        current.status === "ready"
          ? { ...current, streamStatus: "failed", streamError: String(error) }
          : current,
      );
    });
  }, [selectedSessionId, streamStatus, ensureDisplaySubscription]);

  const selectSession = useCallback(
    async (sessionId: string) => {
      if (!client) {
        return;
      }
      selectedSessionIdRef.current = sessionId;
      setInteraction((current) =>
        current.status === "ready"
          ? { ...current, selectedSessionId: sessionId, refreshing: true }
          : current,
      );
      try {
        const selectedSession = await client.getSession(sessionId);
        setInteraction((current) =>
          current.status === "ready"
            ? {
                ...current,
                selectedSessionId: selectedSession.sessionId,
                selectedSession,
                conversation: conversationFromSessionSnapshot(selectedSession),
                refreshing: false,
              }
            : current,
        );
      } catch (error) {
        setInteraction({ status: "failed", error: String(error) });
      }
    },
    [client],
  );

  const createSession = useCallback(async () => {
    const ready = readyRef.current;
    if (!client || !ready) {
      return null;
    }

    setInteraction((current) =>
      current.status === "ready" ? { ...current, refreshing: true } : current,
    );
    try {
      const profileId = ready.initializeResult.profiles?.[0]?.profileId;
      const created = await client.createSession({ profileId });
      const sessions = await listSessionsForProfiles(client, ready.initializeResult);
      const selectedSession = await client.getSession(created.sessionId);
      selectedSessionIdRef.current = selectedSession.sessionId;
      setInteraction((current) =>
        current.status === "ready"
          ? {
              ...current,
              sessions,
              selectedSessionId: selectedSession.sessionId,
              selectedSession,
              conversation: conversationFromSessionSnapshot(selectedSession),
              refreshing: false,
            }
          : current,
      );
      return selectedSession.sessionId;
    } catch (error) {
      setInteraction({ status: "failed", error: String(error) });
      return null;
    }
  }, [client]);

  const submitPrompt = useCallback(
    async (submission: PromptSubmission) => {
      const ready = readyRef.current;
      if (!client || !ready) {
        return;
      }
      const promptText = optimisticPromptText(submission);
      const content = submissionContentBlocks(submission);
      if (content.length === 0) {
        return;
      }

      try {
        let sessionId = ready.selectedSessionId;
        if (!sessionId) {
          sessionId = await createSession();
        }
        if (!sessionId) {
          return;
        }
        selectedSessionIdRef.current = sessionId;

        await ensureDisplaySubscription(sessionId);
        const stream = streamRef.current;
        if (!stream) {
          throw new Error(i18n.t("runtime.disconnected"));
        }

        setInteraction((current) =>
          current.status === "ready"
            ? {
                ...current,
                selectedSessionId: sessionId,
                conversation: appendLocalPrompt(current.conversation, promptText),
                sending: true,
                streamError: null,
              }
            : current,
        );

        const promptedSession = await stream.prompt({
          sessionId,
          input:
            submission.attachments.length === 0
              ? { type: "text", text: submission.text.trimEnd() }
              : {
                  type: "message",
                  message: {
                    id: `app-prompt-${Date.now()}`,
                    role: "user",
                    content,
                    metadata: {},
                  },
                },
        });
        const selectedSession = await client.getSession(promptedSession.sessionId);
        const latestReady = readyRef.current;
        const sessions = await listSessionsForProfiles(
          client,
          latestReady?.initializeResult ?? ready.initializeResult,
        );
        setInteraction((current) =>
          current.status === "ready"
            ? {
                ...current,
                sessions,
                selectedSessionId: selectedSession.sessionId,
                selectedSession,
                conversation: convergeConversationToSessionSnapshot(
                  current.conversation,
                  selectedSession,
                ),
                sending: false,
                refreshing: false,
              }
            : current,
        );
      } catch (error) {
        setInteraction((current) =>
          current.status === "ready"
            ? {
                ...current,
                sending: false,
                streamStatus: streamRef.current ? current.streamStatus : "failed",
                streamError: String(error),
              }
            : { status: "failed", error: String(error) },
        );
      }
    },
    [client, createSession, ensureDisplaySubscription, i18n],
  );

  const abortCurrentRun = useCallback(async () => {
    const ready = readyRef.current;
    if (!ready?.selectedSessionId) {
      return;
    }
    const stream = streamRef.current;
    if (!stream) {
      setInteraction((current) =>
        current.status === "ready"
          ? { ...current, streamError: i18n.t("runtime.disconnected") }
          : current,
      );
      return;
    }
    try {
      const selectedSession = await stream.abort({
        sessionId: ready.selectedSessionId,
      });
      setInteraction((current) =>
        current.status === "ready"
          ? {
              ...current,
              selectedSession,
              conversation: convergeConversationToSessionSnapshot(
                current.conversation,
                selectedSession,
              ),
              sending: false,
            }
          : current,
      );
    } catch (error) {
      setInteraction((current) =>
        current.status === "ready" ? { ...current, streamError: String(error) } : current,
      );
    }
  }, [i18n]);

  const resolveApproval = useCallback(
    async (approvalId: string, outcome: AppToolPermissionOutcome) => {
      if (resolvingApprovalIdsRef.current.has(approvalId)) {
        return;
      }
      const ready = readyRef.current;
      if (!ready?.selectedSessionId) {
        return;
      }
      const stream = streamRef.current;
      if (!stream) {
        setInteraction((current) =>
          current.status === "ready"
            ? { ...current, streamError: i18n.t("runtime.disconnected") }
            : current,
        );
        return;
      }
      resolvingApprovalIdsRef.current.add(approvalId);
      setResolvingApprovalIds(new Set(resolvingApprovalIdsRef.current));
      try {
        await stream.resolveApproval({
          sessionId: ready.selectedSessionId,
          approvalId,
          decision: {
            outcome,
            approver: "noloong-app",
          },
        });
        setInteraction((current) =>
          current.status === "ready"
            ? {
                ...current,
                conversation: markApprovalResolved(current.conversation, approvalId, outcome),
              }
            : current,
        );
      } catch (error) {
        setInteraction((current) =>
          current.status === "ready" ? { ...current, streamError: String(error) } : current,
        );
      } finally {
        resolvingApprovalIdsRef.current.delete(approvalId);
        setResolvingApprovalIds(new Set(resolvingApprovalIdsRef.current));
      }
    },
    [i18n],
  );

  const toggleReasoning = useCallback((thoughtId: string, expanded: boolean) => {
    setInteraction((current) =>
      current.status === "ready"
        ? {
            ...current,
            conversation: setReasoningExpanded(current.conversation, thoughtId, expanded),
          }
        : current,
    );
  }, []);

  return {
    interaction,
    resolvingApprovalIds,
    createSession,
    selectSession,
    submitPrompt,
    abortCurrentRun,
    resolveApproval,
    toggleReasoning,
  };
}
