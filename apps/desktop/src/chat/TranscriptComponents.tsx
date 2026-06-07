import { useCallback, useEffect, useRef, useState } from "react";
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
import { isNearTranscriptBottom, scrollTranscriptToEnd } from "./scroll";
import { sessionTitle } from "./sessionHelpers";
import type { InteractionState } from "./types";

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
              <span>
                {session.profileId} · {session.status}
              </span>
            </button>
          </li>
        );
      })}
    </ul>
  );
}

export function RuntimeBanner({
  i18n,
  interaction,
  profileConfigPath,
}: {
  i18n: AppI18n;
  interaction: InteractionState;
  profileConfigPath: string;
}) {
  if (interaction.status !== "ready") {
    return null;
  }

  return (
    <div className="runtime-banner">
      <span>
        {interaction.initializeResult.server.name} ·{" "}
        {interaction.initializeResult.server.protocolVersion}
      </span>
      <span>{profileConfigPath}</span>
      <span className={interaction.streamStatus === "ready" ? "stream-ok" : "stream-warn"}>
        {i18n.streamStatus(interaction.streamStatus, interaction.streamError)}
      </span>
    </div>
  );
}

export function TranscriptView({
  i18n,
  interaction,
  onAbortRun,
  onOpenSettings,
  onResolveApproval,
  onSubmitPrompt,
  onToggleReasoning,
}: {
  i18n: AppI18n;
  interaction: InteractionState;
  onAbortRun: () => Promise<void>;
  onOpenSettings: () => void;
  onResolveApproval: (approvalId: string, outcome: AppToolPermissionOutcome) => Promise<void>;
  onSubmitPrompt: (text: string) => Promise<void>;
  onToggleReasoning: (thoughtId: string, expanded: boolean) => void;
}) {
  const transcriptRef = useRef<HTMLDivElement | null>(null);
  const transcriptEndRef = useRef<HTMLDivElement | null>(null);
  const shouldStickToBottomRef = useRef(true);

  useEffect(() => {
    const transcript = transcriptRef.current;
    const end = transcriptEndRef.current;
    if (!transcript || !end || !shouldStickToBottomRef.current) {
      return;
    }
    requestAnimationFrame(() => {
      scrollTranscriptToEnd(transcript, end);
    });
  }, [interaction]);

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
        <button className="text-button primary" onClick={onOpenSettings} type="button">
          {i18n.t("chat.openSettings")}
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
  const title = interaction.selectedSession
    ? sessionTitle(interaction.selectedSession)
    : i18n.t("transcript.newSessionTitle");
  const subtitle = interaction.selectedSession
    ? `${interaction.selectedSession.profileId} · ${interaction.selectedSession.status}`
    : i18n.t("transcript.newSessionDetail");

  return (
    <div className="conversation">
      <div
        className="transcript"
        onScroll={(event) => {
          shouldStickToBottomRef.current = isNearTranscriptBottom(event.currentTarget);
        }}
        ref={transcriptRef}
      >
        <div className="session-title-row">
          <div>
            <h1>{title}</h1>
            <p>{subtitle}</p>
          </div>
          <RunStatusPill
            canAbort={canAbort}
            conversation={interaction.conversation}
            i18n={i18n}
            refreshing={interaction.refreshing}
            onAbortRun={onAbortRun}
          />
        </div>
        {interaction.conversation.timeline.length === 0 ? (
          <p className="muted">{i18n.t("transcript.empty")}</p>
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
        <div aria-hidden="true" className="transcript-end" ref={transcriptEndRef} />
      </div>
      <PromptComposer
        disabled={!canSubmit}
        i18n={i18n}
        onSubmit={onSubmitPrompt}
        placeholder={
          interaction.streamStatus === "ready"
            ? i18n.t("composer.write")
            : interaction.streamError ?? i18n.t("composer.connecting")
        }
        sending={interaction.sending}
      />
    </div>
  );
}

function RunStatusPill({
  canAbort,
  conversation,
  i18n,
  refreshing,
  onAbortRun,
}: {
  canAbort: boolean;
  conversation: ConversationState;
  i18n: AppI18n;
  refreshing: boolean;
  onAbortRun: () => Promise<void>;
}) {
  const label = refreshing ? i18n.t("run.refreshing") : runStatusLabel(conversation, i18n);
  return (
    <div className="run-status">
      <span className={`pill run-${conversation.runStatus}`}>{label}</span>
      {canAbort ? (
        <button className="stop-button" onClick={() => void onAbortRun()} type="button">
          {i18n.t("run.stop")}
        </button>
      ) : null}
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
      return (
        <ReasoningCard i18n={i18n} thought={item} onToggleReasoning={onToggleReasoning} />
      );
    case "tool":
      return <ToolActivityRow tool={item} />;
    case "approval":
      return <ApprovalCard approval={item} i18n={i18n} onResolveApproval={onResolveApproval} />;
  }
}

function MessageCard({ i18n, item }: { i18n: AppI18n; item: MessageTimelineItem }) {
  return (
    <article className={`message ${item.role}`}>
      <div className="message-role">{item.pending ? i18n.t("message.sending") : item.role}</div>
      <p>{item.text}</p>
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
  const hasDetails = summary.length > 0 || thought.rawText.length > 0;
  const showDetails = thought.status === "running" || thought.expanded;
  const canToggleDetails =
    thought.status === "completed"
      ? hasDetails
      : thought.rawText.length > 0 && thought.summaryText.length > 0;
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
          <p>{summary}</p>
        ) : (
          <p className="muted">{i18n.t("reasoning.empty")}</p>
        )
      ) : null}
      {thought.expanded && thought.rawText ? <pre>{thought.rawText}</pre> : null}
    </article>
  );
}

function ToolActivityRow({ tool }: { tool: ToolTimelineItem }) {
  const detail = tool.outputText || tool.updates.at(-1) || "";
  return (
    <article className={`activity-card tool-card ${tool.isError ? "tool-error" : ""}`}>
      <div className="activity-title-row">
        <span>{tool.toolName}</span>
        <span>{tool.status}</span>
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
  return (
    <article className="activity-card approval-card">
      <div className="activity-title-row">
        <span>{i18n.t("approval.required")}</span>
        <span>{approval.status}</span>
      </div>
      <p>
        <strong>{approval.toolName}</strong>
        {approval.prompt ? ` · ${approval.prompt}` : ""}
      </p>
      {approval.reason ? <p>{approval.reason}</p> : null}
      {approval.permissionDescriptions.length > 0 ? (
        <ul>
          {approval.permissionDescriptions.map((permission) => (
            <li key={permission}>{permission}</li>
          ))}
        </ul>
      ) : null}
      {pending ? (
        <div className="approval-actions">
          <button
            onClick={() => void onResolveApproval(approval.approvalId, "allow")}
            type="button"
          >
            {i18n.t("approval.allow")}
          </button>
          <button
            onClick={() => void onResolveApproval(approval.approvalId, "deny")}
            type="button"
          >
            {i18n.t("approval.deny")}
          </button>
        </div>
      ) : null}
    </article>
  );
}

function PromptComposer({
  disabled,
  i18n,
  onSubmit,
  placeholder,
  sending,
}: {
  disabled: boolean;
  i18n: AppI18n;
  onSubmit: (text: string) => Promise<void>;
  placeholder: string;
  sending: boolean;
}) {
  const [text, setText] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const canSend = text.trim().length > 0 && !disabled;

  const submit = useCallback(async () => {
    if (!canSend) {
      return;
    }
    const submitted = text;
    setText("");
    await onSubmit(submitted);
  }, [canSend, onSubmit, text]);

  return (
    <form
      className="composer"
      onClick={() => textareaRef.current?.focus()}
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
    >
      <textarea
        disabled={disabled}
        onChange={(event) => setText(event.target.value)}
        onKeyDown={(event) => {
          if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
            event.preventDefault();
            void submit();
          }
        }}
        placeholder={placeholder}
        ref={textareaRef}
        rows={3}
        value={text}
      />
      <div className="composer-footer">
        <span>{sending ? i18n.t("composer.running") : i18n.t("composer.shortcut")}</span>
        <button className="send-button" disabled={!canSend} type="submit">
          ↑
        </button>
      </div>
    </form>
  );
}

function runStatusLabel(conversation: ConversationState, i18n: AppI18n): string {
  if (conversation.runStatus === "failed" && conversation.runError) {
    return i18n.runStatus("failed", conversation.runError);
  }
  if (conversation.runStatus === "paused" && conversation.pauseReason) {
    return i18n.runStatus("paused", conversation.pauseReason);
  }
  return i18n.runStatus(conversation.runStatus);
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
