import { ShieldCheck, Settings } from "lucide-react";
import { useCallback, useEffect, useRef } from "react";
import type {
  AppToolPermissionOutcome,
} from "../generated/contracts";
import {
  reasoningVisibleText,
  type ApprovalTimelineItem,
  type ConversationState,
  type MessageTimelineItem,
  type ReasoningTimelineItem,
  type TimelineItem,
  type ToolTimelineItem,
} from "../interaction/conversationState";
import type { AppI18n } from "../i18n";
import { CenteredStatus } from "./CenteredStatus";
import { MarkdownMessage } from "../markdown/MarkdownMessage";
import { MarkdownRenderer } from "../markdown/MarkdownRenderer";
import { isNearTranscriptBottom, scrollTranscriptToEnd } from "./scroll";
import { sessionContextLabel, sessionTitle } from "./sessionHelpers";
import type { InteractionState } from "./types";
import type { PromptSubmission } from "./attachments";
import type { ConversationMenuState } from "./conversationCommands";
import { PromptComposer } from "./PromptComposer";

export function SessionList({
  i18n,
  interaction,
  onSelect,
}: {
  i18n: AppI18n;
  interaction: InteractionState;
  onSelect: (sessionId: string) => Promise<void>;
}) {
  if (interaction.status === "loading") {
    return <p className="muted">{i18n.t("sessions.loading")}</p>;
  }
  if (interaction.status === "disconnected") {
    return <p className="muted">{i18n.disconnected(interaction.launchStatus).detail}</p>;
  }
  if (interaction.status === "failed") {
    return <p className="error-text">{interaction.error}</p>;
  }
  if (interaction.sessions.length === 0) {
    return <p className="muted">{i18n.t("sessions.empty")}</p>;
  }

  return (
    <ul className="session-list">
      {interaction.sessions.map((session) => {
        const selected = session.sessionId === interaction.selectedSessionId;
        return (
          <li key={session.sessionId}>
            <button
              className={selected ? "session-card selected" : "session-card"}
              onClick={() => void onSelect(session.sessionId)}
              type="button"
            >
              <strong>{sessionTitle(session)}</strong>
              <span>{sessionContextLabel(session, interaction.initializeResult.profiles, i18n)}</span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

export function TranscriptView({
  i18n,
  interaction,
  onAbortRun,
  onConversationMenuStateChange,
  onCreateSession,
  onOpenSettings,
  onOpenSessions,
  onResolveApproval,
  onSubmitPrompt,
  onToggleReasoning,
}: {
  i18n: AppI18n;
  interaction: InteractionState;
  onAbortRun: () => Promise<void>;
  onConversationMenuStateChange: (state: ConversationMenuState) => void;
  onCreateSession: () => void;
  onOpenSettings: () => void;
  onOpenSessions: () => void;
  onResolveApproval: (approvalId: string, outcome: AppToolPermissionOutcome) => Promise<void>;
  onSubmitPrompt: (submission: PromptSubmission) => Promise<void>;
  onToggleReasoning: (thoughtId: string, expanded: boolean) => void;
}) {
  const transcriptRef = useRef<HTMLDivElement | null>(null);
  const transcriptContentRef = useRef<HTMLDivElement | null>(null);
  const shouldStickToBottomRef = useRef(true);
  const pendingScrollFrameRef = useRef(false);
  const programmaticScrollRef = useRef(false);
  const programmaticScrollTokenRef = useRef(0);
  const lastScrollTopRef = useRef(0);

  const stickTranscriptToBottom = useCallback(() => {
    const transcript = transcriptRef.current;
    if (!transcript || !shouldStickToBottomRef.current) {
      return;
    }
    if (pendingScrollFrameRef.current) {
      return;
    }
    pendingScrollFrameRef.current = true;
    requestAnimationFrame(() => {
      pendingScrollFrameRef.current = false;
      const currentTranscript = transcriptRef.current;
      if (!currentTranscript || !shouldStickToBottomRef.current) {
        return;
      }
      const token = programmaticScrollTokenRef.current + 1;
      programmaticScrollTokenRef.current = token;
      programmaticScrollRef.current = true;
      scrollTranscriptToEnd(currentTranscript);
      lastScrollTopRef.current = currentTranscript.scrollTop;
      requestAnimationFrame(() => {
        if (programmaticScrollTokenRef.current === token) {
          programmaticScrollRef.current = false;
        }
      });
    });
  }, []);

  useEffect(() => {
    stickTranscriptToBottom();
  }, [interaction, stickTranscriptToBottom]);

  useEffect(() => {
    const content = transcriptContentRef.current;
    if (!content || typeof ResizeObserver === "undefined") {
      return;
    }

    const observer = new ResizeObserver(() => {
      stickTranscriptToBottom();
    });
    observer.observe(content);
    return () => {
      observer.disconnect();
    };
  }, [interaction.status, stickTranscriptToBottom]);

  const submitPrompt = useCallback(
    async (submission: PromptSubmission) => {
      shouldStickToBottomRef.current = true;
      await onSubmitPrompt(submission);
    },
    [onSubmitPrompt],
  );

  if (interaction.status === "loading") {
    return (
      <CenteredStatus
        title={i18n.t("chat.connectingTitle")}
        detail={i18n.t("chat.connectingDetail")}
      />
    );
  }
  if (interaction.status === "disconnected") {
    const disconnected = i18n.disconnected(interaction.launchStatus);
    return (
      <CenteredStatus title={disconnected.title} detail={disconnected.detail}>
        <button className="text-button primary icon-text" onClick={onOpenSettings} type="button">
          <Settings size={16} />
          <span>{i18n.t("chat.openSettings")}</span>
        </button>
      </CenteredStatus>
    );
  }
  if (interaction.status === "failed") {
    return <CenteredStatus title={i18n.t("chat.failedTitle")} detail={interaction.error} />;
  }

  const canSubmit = interaction.streamStatus === "ready" && !interaction.sending;
  const canAbort =
    interaction.conversation.runStatus === "running" ||
    interaction.conversation.runStatus === "paused";
  const selectedMessages = interaction.selectedSession?.state.messages ?? [];
  const title =
    interaction.selectedSession && selectedMessages.length > 0
      ? sessionTitle(interaction.selectedSession)
      : i18n.t("transcript.newSessionTitle");
  const subtitle = interaction.selectedSession
    ? sessionContextLabel(interaction.selectedSession, interaction.initializeResult.profiles, i18n)
    : i18n.t("transcript.newSessionDetail");
  const timelineEmpty = interaction.conversation.timeline.length === 0;

  return (
    <div className="conversation">
      <div
        className="transcript"
        onScroll={(event) => {
          const transcript = event.currentTarget;
          const previousScrollTop = lastScrollTopRef.current;
          const currentScrollTop = transcript.scrollTop;
          lastScrollTopRef.current = currentScrollTop;

          if (programmaticScrollRef.current) {
            return;
          }
          if (currentScrollTop < previousScrollTop - 1) {
            shouldStickToBottomRef.current = false;
            return;
          }
          if (isNearTranscriptBottom(transcript)) {
            shouldStickToBottomRef.current = true;
          }
        }}
        ref={transcriptRef}
      >
        <div
          className={timelineEmpty ? "transcript-content transcript-content-empty" : "transcript-content"}
          ref={transcriptContentRef}
        >
          {timelineEmpty ? (
            <div className="empty-session-heading">
              <h1 data-render-heading>{title}</h1>
              <p>{subtitle}</p>
            </div>
          ) : null}
          {timelineEmpty ? (
            <p className="transcript-empty-prompt">{i18n.t("transcript.empty")}</p>
          ) : (
            interaction.conversation.timeline.map((item) => (
              <TimelineItemView
                i18n={i18n}
                item={item}
                key={timelineItemKey(item)}
                onResolveApproval={onResolveApproval}
                onToggleReasoning={onToggleReasoning}
              />
            ))
          )}
          <div aria-hidden="true" className="transcript-end" />
        </div>
      </div>
      <PromptComposer
        disabled={!canSubmit}
        i18n={i18n}
        onAbortRun={canAbort ? onAbortRun : undefined}
        onCommandAvailabilityChange={onConversationMenuStateChange}
        onCreateSession={onCreateSession}
        onOpenSessions={onOpenSessions}
        onSubmit={submitPrompt}
        placeholder={
          interaction.streamStatus === "ready"
            ? i18n.t("composer.write")
            : interaction.streamError ?? i18n.t("composer.connecting")
        }
      />
    </div>
  );
}

function TimelineItemView({
  i18n,
  item,
  onResolveApproval,
  onToggleReasoning,
}: {
  i18n: AppI18n;
  item: TimelineItem;
  onResolveApproval: (approvalId: string, outcome: AppToolPermissionOutcome) => Promise<void>;
  onToggleReasoning: (thoughtId: string, expanded: boolean) => void;
}) {
  switch (item.kind) {
    case "message":
      return <MessageCard i18n={i18n} item={item} />;
    case "reasoning":
      if (item.status === "completed") {
        return null;
      }
      return (
        <ReasoningCard i18n={i18n} thought={item} onToggleReasoning={onToggleReasoning} />
      );
    case "tool":
      return <ToolActivityRow i18n={i18n} tool={item} />;
    case "approval":
      return <ApprovalCard approval={item} i18n={i18n} onResolveApproval={onResolveApproval} />;
  }
}

function MessageCard({ i18n, item }: { i18n: AppI18n; item: MessageTimelineItem }) {
  return (
    <article className={`message ${item.role}${item.pending ? " pending" : ""}`}>
      <div className="message-role">{item.pending ? i18n.t("message.sending") : item.role}</div>
      <MarkdownMessage role={item.role} streaming={Boolean(item.live)} text={item.text} />
    </article>
  );
}

function ReasoningCard({
  i18n,
  thought,
  onToggleReasoning,
}: {
  i18n: AppI18n;
  thought: ReasoningTimelineItem;
  onToggleReasoning: (thoughtId: string, expanded: boolean) => void;
}) {
  const summary = reasoningVisibleText(thought);
  const rawText = thought.rawText;
  const hasRawText = rawText.length > 0;
  const showDetails = thought.status === "running";
  const canToggleDetails =
    thought.status === "completed" ? hasRawText : hasRawText && thought.summaryText.length > 0;
  const title =
    thought.status === "completed"
      ? i18n.t("reasoning.thoughtFor", { duration: i18n.duration(thought.elapsedMs) })
      : i18n.t("reasoning.thinking");

  return (
    <article className="activity-card reasoning-card">
      <div className="activity-title-row">
        <span>{title}</span>
        {canToggleDetails ? (
          <button
            className="activity-link"
            onClick={() => onToggleReasoning(thought.thoughtId, !thought.expanded)}
            type="button"
          >
            {thought.expanded ? i18n.t("reasoning.hideRaw") : i18n.t("reasoning.showRaw")}
          </button>
        ) : null}
      </div>
      {showDetails ? (
        summary ? (
          <div className="reasoning-content">
            <MarkdownRenderer streaming={thought.status === "running"}>{summary}</MarkdownRenderer>
          </div>
        ) : (
          <p className="muted">{i18n.t("reasoning.empty")}</p>
        )
      ) : null}
      {thought.expanded && rawText ? (
        <div className="reasoning-raw">
          <MarkdownRenderer>{rawText}</MarkdownRenderer>
        </div>
      ) : null}
    </article>
  );
}

function ToolActivityRow({ i18n, tool }: { i18n: AppI18n; tool: ToolTimelineItem }) {
  const detail = tool.outputText || tool.updates.at(-1) || "";
  return (
    <article className={`activity-card tool-card ${tool.isError ? "tool-error" : ""}`}>
      <div className="activity-title-row">
        <span>{tool.toolName}</span>
        <span className={`activity-status activity-status-${tool.status}`}>
          {tool.status === "running" ? i18n.t("tool.running") : i18n.t("tool.done")}
        </span>
      </div>
      {detail ? <p>{detail}</p> : null}
    </article>
  );
}

function ApprovalCard({
  approval,
  i18n,
  onResolveApproval,
}: {
  approval: ApprovalTimelineItem;
  i18n: AppI18n;
  onResolveApproval: (approvalId: string, outcome: AppToolPermissionOutcome) => Promise<void>;
}) {
  const pending = approval.status === "pending";
  const decision = approvalDecisionViewModel(approval, i18n);
  return (
    <article
      aria-label={i18n.t("approval.required")}
      className={`activity-card approval-card approval-card-${approval.status}`}
    >
      <header className="approval-head">
        <span aria-hidden="true" className="approval-glyph">
          <ShieldCheck size={16} />
        </span>
        <div>
          <p className="approval-eyebrow">{approvalStatusLabel(approval.status, i18n)}</p>
          <h2>{decision.title}</h2>
        </div>
      </header>
      {decision.command ? <p className="approval-command">{decision.command}</p> : null}
      {decision.reason ? <p className="approval-reason">{decision.reason}</p> : null}
      <dl className="approval-impact">
        <div>
          <dt>{i18n.t("approval.tool")}</dt>
          <dd>{approval.toolName}</dd>
        </div>
        {approval.cwd ? (
          <div>
            <dt>{i18n.t("approval.directory")}</dt>
            <dd>
              <code>{approval.cwd}</code>
            </dd>
          </div>
        ) : null}
        {decision.permissions.length > 0 ? (
          <div>
            <dt>{i18n.t("approval.permissions")}</dt>
            <dd>
              <ul className="approval-permission-list">
                {decision.permissions.map((permission) => (
                  <li key={permission}>{permission}</li>
                ))}
              </ul>
            </dd>
          </div>
        ) : null}
      </dl>
      {pending ? (
        <div className="approval-actions">
          <button
            className="approval-deny"
            onClick={() => void onResolveApproval(approval.approvalId, "deny")}
            type="button"
          >
            {i18n.t("approval.deny")}
          </button>
          <button
            className="approval-allow"
            onClick={() => void onResolveApproval(approval.approvalId, "allow")}
            type="button"
          >
            {i18n.t("approval.allow")}
          </button>
        </div>
      ) : null}
    </article>
  );
}

function approvalDecisionViewModel(approval: ApprovalTimelineItem, i18n: AppI18n) {
  const prompt = approval.prompt.trim();
  const reason = approval.reason || (approval.command ? "" : prompt);
  return {
    title: approval.command ? i18n.t("approval.commandTitle") : i18n.t("approval.actionTitle"),
    command: approval.command,
    reason,
    permissions: approval.permissionDescriptions,
  };
}

function approvalStatusLabel(
  status: ApprovalTimelineItem["status"],
  i18n: AppI18n,
): string {
  switch (status) {
    case "pending":
      return i18n.t("approval.pending");
    case "approved":
      return i18n.t("approval.approved");
    case "denied":
      return i18n.t("approval.denied");
    case "expired":
      return i18n.t("approval.expired");
  }
}

function timelineItemKey(item: TimelineItem): string {
  switch (item.kind) {
    case "message":
      return `message:${item.id}`;
    case "reasoning":
      return `reasoning:${item.thoughtId}`;
    case "tool":
      return `tool:${item.toolCallId}`;
    case "approval":
      return `approval:${item.approvalId}`;
  }
}
