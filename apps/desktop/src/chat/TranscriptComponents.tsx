import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { Maximize2, Minimize2, Paperclip, Send, Settings, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
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
import { sessionTitle } from "./sessionHelpers";
import type { InteractionState } from "./types";
import {
  pathsToAttachments,
  type PromptAttachment,
  type PromptSubmission,
} from "./attachments";

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
    ? `${interaction.selectedSession.profileId} · ${interaction.selectedSession.status}`
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
            <div className="session-title-row">
              <div>
                <h1 data-render-heading>{title}</h1>
                <p>{subtitle}</p>
              </div>
              {canAbort ? <RunControl i18n={i18n} onAbortRun={onAbortRun} /> : null}
            </div>
          ) : (
            canAbort ? (
              <div className="session-status-row">
                <RunControl i18n={i18n} onAbortRun={onAbortRun} />
              </div>
            ) : null
          )}
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

function RunControl({
  i18n,
  onAbortRun,
}: {
  i18n: AppI18n;
  onAbortRun: () => Promise<void>;
}) {
  return (
    <div className="run-status">
      <button className="stop-button" onClick={() => void onAbortRun()} type="button">
        {i18n.t("run.stop")}
      </button>
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
  const summary = approval.prompt.trim();
  return (
    <article aria-label={i18n.t("approval.required")} className="activity-card approval-card">
      <div className="activity-title-row">
        <span>{i18n.t("approval.required")}</span>
        <span className={`approval-status approval-status-${approval.status}`}>
          {approvalStatusLabel(approval.status, i18n)}
        </span>
      </div>
      <dl className="approval-details">
        <div>
          <dt>{i18n.t("approval.tool")}</dt>
          <dd>{approval.toolName}</dd>
        </div>
        {approval.command ? (
          <div>
            <dt>{i18n.t("approval.command")}</dt>
            <dd className="approval-command">{approval.command}</dd>
          </div>
        ) : null}
        {approval.cwd ? (
          <div>
            <dt>{i18n.t("approval.directory")}</dt>
            <dd>{approval.cwd}</dd>
          </div>
        ) : null}
        {approval.reason ? (
          <div>
            <dt>{i18n.t("approval.reason")}</dt>
            <dd>{approval.reason}</dd>
          </div>
        ) : null}
        {!approval.command && !approval.cwd && summary ? (
          <div>
            <dt>{i18n.t("approval.reason")}</dt>
            <dd>{summary}</dd>
          </div>
        ) : null}
      </dl>
      {approval.permissionDescriptions.length > 0 ? (
        <section className="approval-permissions">
          <h3>{i18n.t("approval.permissions")}</h3>
          <ul>
            {approval.permissionDescriptions.map((permission) => (
              <li key={permission}>{permission}</li>
            ))}
          </ul>
        </section>
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

function PromptComposer({
  disabled,
  i18n,
  onSubmit,
  placeholder,
}: {
  disabled: boolean;
  i18n: AppI18n;
  onSubmit: (submission: PromptSubmission) => Promise<void>;
  placeholder: string;
}) {
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<PromptAttachment[]>([]);
  const [dragging, setDragging] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [contentOverflowing, setContentOverflowing] = useState(false);
  const formRef = useRef<HTMLFormElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const canSend = (text.trim().length > 0 || attachments.length > 0) && !disabled;
  const canExpand = expanded || contentOverflowing;
  const compactPreview = text.split(/\r?\n/, 1)[0] ?? text;

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    if (!textarea) {
      return;
    }
    textarea.style.height = "38px";
    if (!expanded) {
      textarea.scrollTop = 0;
    }
    const overflowing =
      text.length > 0 &&
      (textarea.scrollHeight > textarea.clientHeight + 1 ||
        textarea.scrollWidth > textarea.clientWidth + 1);
    setContentOverflowing(overflowing);
  }, [expanded, text]);

  const submit = useCallback(async () => {
    if (!canSend) {
      return;
    }
    const submitted = text;
    const submittedAttachments = attachments;
    setText("");
    setAttachments([]);
    setExpanded(false);
    setContentOverflowing(false);
    await onSubmit({ text: submitted, attachments: submittedAttachments });
  }, [attachments, canSend, onSubmit, text]);

  const addPaths = useCallback((paths: string[]) => {
    setAttachments((current) => {
      const existing = new Set(current.map((attachment) => attachment.path));
      return [
        ...current,
        ...pathsToAttachments(paths).filter((attachment) => !existing.has(attachment.path)),
      ];
    });
  }, []);

  const pickFiles = useCallback(async () => {
    const selected = await open({ multiple: true, directory: false });
    if (!selected) {
      return;
    }
    addPaths(Array.isArray(selected) ? selected : [selected]);
  }, [addPaths]);

  const isComposerDropPosition = useCallback((position: { x: number; y: number }) => {
    const form = formRef.current;
    if (!form) {
      return false;
    }
    const scale = window.devicePixelRatio || 1;
    const x = position.x / scale;
    const y = position.y / scale;
    const rect = form.getBoundingClientRect();
    return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          switch (event.payload.type) {
            case "enter":
            case "over":
              setDragging(isComposerDropPosition(event.payload.position));
              break;
            case "drop":
              setDragging(false);
              if (isComposerDropPosition(event.payload.position)) {
                addPaths(event.payload.paths);
              }
              break;
            case "leave":
              setDragging(false);
              break;
          }
        })
        .then((dispose) => {
          unlisten = dispose;
        })
        .catch(() => {
          unlisten = null;
        });
    } catch {
      unlisten = null;
    }
    return () => {
      unlisten?.();
    };
  }, [addPaths]);

  return (
    <form
      className={[
        "composer",
        dragging ? "dragging" : "",
        expanded ? "expanded" : "",
        canExpand ? "can-expand" : "",
        attachments.length > 0 ? "has-attachments" : "",
      ]
        .filter(Boolean)
        .join(" ")}
      onClick={() => textareaRef.current?.focus()}
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
      ref={formRef}
    >
      {!expanded && canExpand ? (
        <span aria-hidden="true" className="composer-preview">
          {compactPreview}
        </span>
      ) : null}
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
        rows={1}
        value={text}
      />
      {canExpand ? (
        <button
          aria-label={i18n.t(expanded ? "composer.collapse" : "composer.expand")}
          className="composer-expand"
          onClick={(event) => {
            event.preventDefault();
            event.stopPropagation();
            setExpanded((current) => !current);
          }}
          type="button"
        >
          {expanded ? <Minimize2 size={12} /> : <Maximize2 size={12} />}
        </button>
      ) : null}
      {attachments.length > 0 ? (
        <div className="attachment-strip">
          {attachments.map((attachment) => (
            <span className="attachment-pill" key={attachment.path} title={attachment.path}>
              <span className="attachment-name">{attachment.name}</span>
              <button
                aria-label={i18n.t("composer.removeAttachment", { name: attachment.name })}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  setAttachments((current) =>
                    current.filter((item) => item.path !== attachment.path),
                  );
                }}
                type="button"
              >
                <X size={12} />
              </button>
            </span>
          ))}
        </div>
      ) : null}
      <div className="composer-footer">
        <div className="composer-actions">
          <button
            aria-label={i18n.t("composer.attach")}
            className="composer-tool"
            disabled={disabled}
            onClick={(event) => {
              event.stopPropagation();
              void pickFiles();
            }}
            type="button"
          >
            <Paperclip size={16} />
          </button>
          <button aria-label="↑" className="send-button" disabled={!canSend} type="submit">
            <Send size={16} />
          </button>
        </div>
      </div>
    </form>
  );
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
