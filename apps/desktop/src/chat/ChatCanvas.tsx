import {
  connectInteractionDisplayStream as connectDefaultInteractionDisplayStream,
  createInteractionClient as createDefaultInteractionClient,
} from "../interaction/client";
import { MessageCircle, Plus, Settings } from "lucide-react";
import type { AppLaunchOptions } from "../generated/contracts";
import type { AppI18n } from "../i18n";
import { CenteredStatus } from "./CenteredStatus";
import { SessionList, TranscriptView } from "./TranscriptComponents";
import type { BootstrapState, InteractionState } from "./types";
import { useInteractionRuntime } from "./useInteractionRuntime";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";

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
  const ready = runtime.interaction.status === "ready";
  const sessionsPanelVisible = sessionsPanelOpen && ready;
  const surfaceClassName = [
    "chat-surface",
    ready ? "runtime-ready" : "",
    sessionsPanelVisible ? "sessions-panel-open" : "",
  ]
    .filter(Boolean)
    .join(" ");

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
            onOpenSettings={onOpenSettings}
            onOpenSessions={openSessionsPanel}
            onResolveApproval={runtime.resolveApproval}
            onSubmitPrompt={runtime.submitPrompt}
            onToggleReasoning={runtime.toggleReasoning}
          />
        ) : (
          <SurfaceStatus i18n={i18n} interaction={runtime.interaction} onOpenSettings={onOpenSettings} />
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
          <div className="sessions-panel-copy">
            <MessageCircle size={20} />
            <h2 data-render-heading>{i18n.t("sessionsPanel.title")}</h2>
            <p>{i18n.t("sessionsPanel.subtitle")}</p>
          </div>
          <div className="sessions-panel-actions">
            <button
              className="text-button primary icon-text"
              disabled={runtime.interaction.status !== "ready"}
              onClick={() => void runtime.createSession().then(closeSessionsPanel)}
              type="button"
            >
              <Plus size={16} />
              <span>{i18n.t("sessions.create")}</span>
            </button>
            <button
              className="text-button subtle"
              onClick={closeSessionsPanel}
              ref={sessionsPanelCloseRef}
              type="button"
            >
              {i18n.t("sessionsPanel.return")}
            </button>
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

function SurfaceStatus({
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
    <section aria-live="polite" className="surface-status-pill" role="status">
      <h1>{copy.title}</h1>
      <span>{copy.detail}</span>
      {interaction.status === "disconnected" ? (
        <button className="text-button subtle icon-text" onClick={onOpenSettings} type="button">
          <Settings size={14} />
          <span>{i18n.t("chat.prepareEnvironment")}</span>
        </button>
      ) : null}
    </section>
  );
}
