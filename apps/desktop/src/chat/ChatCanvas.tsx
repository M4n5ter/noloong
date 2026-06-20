import {
  connectInteractionDisplayStream as connectDefaultInteractionDisplayStream,
  createInteractionClient as createDefaultInteractionClient,
} from "../interaction/client";
import { Plus, Settings, X } from "lucide-react";
import type { AppLaunchOptions } from "../generated/contracts";
import type { AppI18n } from "../i18n";
import { CenteredStatus } from "./CenteredStatus";
import {
  conversationMenuStateForChatSurface,
  DISABLED_CONVERSATION_MENU_STATE,
  syncConversationMenuState,
  type ConversationMenuState,
} from "./conversationCommands";
import { SessionList, TranscriptView } from "./TranscriptComponents";
import type { BootstrapState, InteractionState } from "./types";
import { useApprovalQuickCancel } from "./useApprovalQuickCancel";
import { useInteractionRuntime } from "./useInteractionRuntime";
import { type KeyboardEvent, useCallback, useEffect, useRef, useState } from "react";

export function ChatCanvas({
  bootstrap,
  connectDisplayStream,
  createInteractionClient,
  i18n,
  onOpenSettings,
}: {
  bootstrap: BootstrapState;
  connectDisplayStream: typeof connectDefaultInteractionDisplayStream;
  createInteractionClient: typeof createDefaultInteractionClient;
  i18n: AppI18n;
  onOpenSettings: () => void;
}) {
  if (bootstrap.status === "loading") {
    return (
      <CenteredStatus
        detail={i18n.t("bootstrap.loadingDetail")}
        title={i18n.t("bootstrap.loadingTitle")}
      />
    );
  }

  if (bootstrap.status === "failed") {
    return <CenteredStatus title={i18n.t("bootstrap.failedTitle")} detail={bootstrap.error} />;
  }

  return (
    <InteractionCanvas
      connectDisplayStream={connectDisplayStream}
      createInteractionClient={createInteractionClient}
      i18n={i18n}
      onOpenSettings={onOpenSettings}
      options={bootstrap.options}
    />
  );
}

function InteractionCanvas({
  connectDisplayStream,
  createInteractionClient,
  i18n,
  onOpenSettings,
  options,
}: {
  connectDisplayStream: typeof connectDefaultInteractionDisplayStream;
  createInteractionClient: typeof createDefaultInteractionClient;
  i18n: AppI18n;
  onOpenSettings: () => void;
  options: AppLaunchOptions;
}) {
  const runtime = useInteractionRuntime({
    connectDisplayStream,
    createInteractionClient,
    i18n,
    options,
  });
  const [sessionsPanelOpen, setSessionsPanelOpen] = useState(false);
  const sessionsPanelRef = useRef<HTMLElement | null>(null);
  const sessionsPanelCloseRef = useRef<HTMLButtonElement | null>(null);
  const previousFocusRef = useRef<HTMLElement | null>(null);
  const [documentMenuTargetAvailable, setDocumentMenuTargetAvailable] = useState(
    isMainDocumentMenuTargetAvailable,
  );
  const [composerMenuState, setComposerMenuState] = useState<ConversationMenuState>(
    DISABLED_CONVERSATION_MENU_STATE,
  );
  const ready = runtime.interaction.status === "ready";
  const sessionsPanelVisible = sessionsPanelOpen && ready;
  const foregroundContextOpen = sessionsPanelVisible;
  const surfaceClassName = [
    "chat-surface",
    ready ? "runtime-ready" : "",
    sessionsPanelVisible ? "sessions-panel-open" : "",
  ]
    .filter(Boolean)
    .join(" ");

  useEffect(() => {
    function updateMenuTargetAvailability() {
      setDocumentMenuTargetAvailable(isMainDocumentMenuTargetAvailable());
    }

    updateMenuTargetAvailability();
    window.addEventListener("focus", updateMenuTargetAvailability);
    window.addEventListener("blur", updateMenuTargetAvailability);
    document.addEventListener("visibilitychange", updateMenuTargetAvailability);
    return () => {
      window.removeEventListener("focus", updateMenuTargetAvailability);
      window.removeEventListener("blur", updateMenuTargetAvailability);
      document.removeEventListener("visibilitychange", updateMenuTargetAvailability);
      syncConversationMenuState(DISABLED_CONVERSATION_MENU_STATE);
    };
  }, []);

  useEffect(() => {
    syncConversationMenuState(conversationMenuStateForChatSurface({
      composerState: composerMenuState,
      documentTargetAvailable: documentMenuTargetAvailable,
      ready,
      sessionsPanelVisible,
    }));
  }, [composerMenuState, documentMenuTargetAvailable, ready, sessionsPanelVisible]);

  useEffect(() => {
    if (ready) {
      return;
    }
    setComposerMenuState(DISABLED_CONVERSATION_MENU_STATE);
  }, [ready]);

  const updateComposerMenuState = useCallback((state: ConversationMenuState) => {
    setComposerMenuState(state);
  }, []);

  useEffect(() => {
    if (!sessionsPanelOpen) {
      return;
    }

    if (!ready) {
      setSessionsPanelOpen(false);
      return;
    }

    const focusTarget = sessionsPanelCloseRef.current ?? sessionsPanelRef.current;
    focusTarget?.focus({ preventScroll: true });
  }, [ready, sessionsPanelOpen]);

  useApprovalQuickCancel({
    conversation: runtime.interaction.status === "ready" ? runtime.interaction.conversation : null,
    disabled: foregroundContextOpen,
    resolvingApprovalIds: runtime.resolvingApprovalIds,
    onResolveApproval: runtime.resolveApproval,
  });

  function openSessionsPanel() {
    previousFocusRef.current =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    setSessionsPanelOpen(true);
  }

  function closeSessionsPanel() {
    setSessionsPanelOpen(false);
    requestAnimationFrame(() => previousFocusRef.current?.focus({ preventScroll: true }));
  }

  function handleSessionsPanelKeyDown(event: KeyboardEvent<HTMLElement>) {
    if (event.key === "Escape") {
      event.preventDefault();
      closeSessionsPanel();
      return;
    }

    if (event.key !== "Tab") {
      return;
    }

    const overlay = sessionsPanelRef.current;
    if (!overlay) {
      return;
    }

    const focusable = Array.from(
      overlay.querySelectorAll<HTMLElement>(
        'button:not([disabled]), [href], input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
      ),
    ).filter((element) => !element.hasAttribute("disabled") && element.getClientRects().length > 0);

    if (focusable.length === 0) {
      event.preventDefault();
      overlay.focus({ preventScroll: true });
      return;
    }

    const first = focusable[0];
    const last = focusable[focusable.length - 1];
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus({ preventScroll: true });
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus({ preventScroll: true });
    }
  }

  async function selectSession(sessionId: string) {
    await runtime.selectSession(sessionId);
    closeSessionsPanel();
  }

  return (
    <section className={surfaceClassName}>
      <section
        aria-hidden={sessionsPanelVisible}
        className={
          ready ? "conversation-surface runtime-ready" : "conversation-surface runtime-unavailable"
        }
        data-render-surface="chat"
        inert={sessionsPanelVisible ? true : undefined}
      >
        {ready ? (
          <TranscriptView
            i18n={i18n}
            interaction={runtime.interaction}
            onAbortRun={runtime.abortCurrentRun}
            onCreateSession={() => void runtime.createSession()}
            onConversationMenuStateChange={updateComposerMenuState}
            onOpenSettings={onOpenSettings}
            onOpenSessions={openSessionsPanel}
            onResolveApproval={runtime.resolveApproval}
            resolvingApprovalIds={runtime.resolvingApprovalIds}
            onSubmitPrompt={runtime.submitPrompt}
            onToggleReasoning={runtime.toggleReasoning}
          />
        ) : (
          <RuntimeRecoveryPanel
            i18n={i18n}
            interaction={runtime.interaction}
            onOpenSettings={onOpenSettings}
          />
        )}
      </section>
      {sessionsPanelVisible ? (
        <section
          aria-label={i18n.t("sessionsPanel.title")}
          aria-modal="true"
          className="sessions-panel"
          data-render-surface="sessions"
          onKeyDown={handleSessionsPanelKeyDown}
          ref={sessionsPanelRef}
          role="dialog"
          tabIndex={-1}
        >
          <div className="sessions-panel-header">
            <h2 data-render-heading>{i18n.t("sessionsPanel.title")}</h2>
            <div className="sessions-panel-tools">
              <button
                aria-label={i18n.t("sessions.create")}
                className="sessions-panel-tool"
                disabled={runtime.interaction.status !== "ready"}
                onClick={() => void runtime.createSession().then(closeSessionsPanel)}
                title={i18n.t("sessions.create")}
                type="button"
              >
                <Plus size={16} />
              </button>
              <button
                aria-label={i18n.t("sessionsPanel.close")}
                className="sessions-panel-tool"
                onClick={closeSessionsPanel}
                ref={sessionsPanelCloseRef}
                title={i18n.t("sessionsPanel.close")}
                type="button"
              >
                <X size={15} />
              </button>
            </div>
          </div>
          <div className="sessions-panel-stack">
            <SessionList
              i18n={i18n}
              interaction={runtime.interaction}
              onSelect={selectSession}
            />
          </div>
        </section>
      ) : null}
    </section>
  );
}

function isMainDocumentMenuTargetAvailable(): boolean {
  return document.visibilityState !== "hidden" && document.hasFocus();
}

function RuntimeRecoveryPanel({
  i18n,
  interaction,
  onOpenSettings,
}: {
  i18n: AppI18n;
  interaction: InteractionState;
  onOpenSettings: () => void;
}) {
  const copy =
    interaction.status === "disconnected"
      ? i18n.disconnected(interaction.launchStatus)
      : interaction.status === "failed"
        ? { title: i18n.t("chat.failedTitle"), detail: interaction.error }
        : { title: i18n.t("chat.connectingTitle"), detail: i18n.t("chat.connectingDetail") };

  return (
    <section aria-live="polite" className="runtime-recovery-panel" role="status">
      <div className="runtime-recovery-copy">
        <h1>{copy.title}</h1>
        <p>{copy.detail}</p>
      </div>
      {interaction.status === "disconnected" ? (
        <button className="text-button subtle icon-text" onClick={onOpenSettings} type="button">
          <Settings size={14} />
          <span>{i18n.t("chat.prepareEnvironment")}</span>
        </button>
      ) : null}
    </section>
  );
}
