import type {
  AppContentBlock,
  AppDisplayEvent,
  AppInteractionSessionDescriptor,
  AppMessage,
  AppToolApprovalRequest,
  AppToolOutput,
  AppToolPermissionDecision,
  AppToolPermissionOutcome,
  AppToolUpdate,
} from "../generated/contracts";
import { readableJson, textFromContentBlocks, textFromMessage } from "./contentText";

export type RunStatus = "idle" | "running" | "paused" | "failed" | "aborted" | "completed";

export type MessageTimelineItem = {
  kind: "message";
  id: string;
  role: string;
  text: string;
  live?: boolean;
  pending?: boolean;
};

export type ReasoningTimelineItem = {
  kind: "reasoning";
  thoughtId: string;
  runId: string;
  status: "running" | "completed";
  summaryText: string;
  rawText: string;
  elapsedMs?: number;
  expanded: boolean;
};

export type ToolTimelineItem = {
  kind: "tool";
  toolCallId: string;
  toolName: string;
  status: "running" | "completed";
  updates: string[];
  outputText: string;
  isError: boolean;
};

export type ApprovalTimelineItem = {
  kind: "approval";
  approvalId: string;
  toolCallId: string;
  toolName: string;
  prompt: string;
  reason: string;
  command: string | null;
  cwd: string | null;
  permissionDescriptions: string[];
  status: "pending" | "approved" | "denied" | "expired";
};

export type TimelineItem =
  | MessageTimelineItem
  | ReasoningTimelineItem
  | ToolTimelineItem
  | ApprovalTimelineItem;

export type ConversationState = {
  timeline: TimelineItem[];
  runStatus: RunStatus;
  currentRunId: string | null;
  runError: string | null;
  pauseReason: string | null;
};

export function emptyConversationState(): ConversationState {
  return {
    timeline: [],
    runStatus: "idle",
    currentRunId: null,
    runError: null,
    pauseReason: null,
  };
}

export function conversationFromSessionSnapshot(
  session: AppInteractionSessionDescriptor | null | undefined,
): ConversationState {
  return {
    ...emptyConversationState(),
    timeline: timelineFromSessionSnapshot(session),
    runStatus: sessionStatusToRunStatus(session?.status),
  };
}

export function appendLocalPrompt(
  state: ConversationState,
  text: string,
  id = `local-user-${Date.now()}`,
): ConversationState {
  return {
    ...state,
    timeline: [
      ...state.timeline,
      {
        kind: "message",
        id,
        role: "user",
        text,
        pending: true,
      },
    ],
  };
}

export function applyDisplayEventToConversation(
  state: ConversationState,
  event: AppDisplayEvent,
): ConversationState {
  switch (event.type) {
    case "run_started":
      return {
        ...state,
        runStatus: "running",
        currentRunId: event.runId,
        runError: null,
        pauseReason: null,
      };
    case "run_completed":
      return {
        ...state,
        timeline: settleRunReasoning(state.timeline, event.runId),
        runStatus: "completed",
        currentRunId: event.runId,
        pauseReason: null,
      };
    case "run_failed":
      return {
        ...state,
        timeline: settleRunReasoning(state.timeline, event.runId),
        runStatus: "failed",
        currentRunId: event.runId,
        runError: event.error,
      };
    case "run_aborted":
      return {
        ...state,
        timeline: settleRunReasoning(state.timeline, event.runId),
        runStatus: "aborted",
        currentRunId: event.runId,
      };
    case "run_paused":
      return {
        ...state,
        runStatus: "paused",
        currentRunId: event.runId,
        pauseReason: readableJson(event.reason),
      };
    case "thought_started":
      return {
        ...state,
        timeline: upsertReasoning(state.timeline, event.thoughtId, (thought) => ({
          ...thought,
          runId: event.runId,
          status: "running",
        })),
      };
    case "thought_delta":
      return {
        ...state,
        timeline: upsertReasoning(state.timeline, event.thoughtId, (thought) => {
          const base = {
            ...thought,
            runId: event.runId,
            status: "running" as const,
          };
          if (event.kind === "summary") {
            return { ...base, summaryText: `${base.summaryText}${event.text}` };
          }
          if (event.kind === "raw") {
            return { ...base, rawText: `${base.rawText}${event.text}` };
          }
          return base;
        }),
      };
    case "thought_completed":
      return {
        ...state,
        timeline: completeReasoning(state.timeline, event.thoughtId, event.runId, event.elapsedMs),
      };
    case "tool_started":
      return {
        ...state,
        timeline: upsertTool(state.timeline, event.toolCallId, (tool) => ({
          ...tool,
          toolName: event.toolName,
          status: "running",
        })),
      };
    case "tool_updated":
      return {
        ...state,
        timeline: upsertTool(state.timeline, event.toolCallId, (tool) => {
          const updateText = textFromToolUpdate(event.update);
          return {
            ...tool,
            status: "running",
            updates: updateText ? [...tool.updates, updateText] : tool.updates,
          };
        }),
      };
    case "tool_completed":
      return {
        ...state,
        timeline: upsertTool(state.timeline, event.toolCallId, (tool) => ({
          ...tool,
          status: "completed",
          outputText: textFromToolOutput(event.output),
          isError: Boolean(event.output.isError),
        })),
      };
    case "approval_requested":
      return {
        ...state,
        timeline: upsertApprovalRequest(state.timeline, event.approval),
      };
    case "approval_resolved":
      return {
        ...state,
        timeline: updateApprovalStatus(
          state.timeline,
          event.approvalId,
          approvalStatusFromDecision(event.decision),
        ),
      };
    case "approval_expired":
      return {
        ...state,
        timeline: updateApprovalStatus(state.timeline, event.approvalId, "expired"),
      };
    case "assistant_message_delta":
      if (isTerminalRunStatus(state.runStatus)) {
        return state;
      }
      return {
        ...state,
        timeline: appendAssistantDelta(state.timeline, event.displayMessageId, event.text),
      };
    case "assistant_message_final":
      return {
        ...state,
        timeline: applyAssistantFinal(state.timeline, event.displayMessageId, event.message),
      };
    default:
      return state;
  }
}

export function convergeConversationToSessionSnapshot(
  state: ConversationState,
  session: AppInteractionSessionDescriptor,
): ConversationState {
  return {
    ...state,
    timeline: convergeTimelineToSessionSnapshot(state.timeline, session),
    runStatus: sessionStatusToRunStatus(session.status),
    currentRunId: state.currentRunId,
    runError: session.status === "failed" ? state.runError : null,
  };
}

export function setReasoningExpanded(
  state: ConversationState,
  thoughtId: string,
  expanded: boolean,
): ConversationState {
  return {
    ...state,
    timeline: state.timeline.map((item) =>
      item.kind === "reasoning" && item.thoughtId === thoughtId
        ? { ...item, expanded }
        : item,
    ),
  };
}

export function markApprovalResolved(
  state: ConversationState,
  approvalId: string,
  outcome: AppToolPermissionOutcome,
): ConversationState {
  return {
    ...state,
    timeline: updateApprovalStatus(
      state.timeline,
      approvalId,
      outcome === "allow" ? "approved" : "denied",
    ),
  };
}

export function reasoningVisibleText(thought: ReasoningTimelineItem): string {
  return thought.summaryText || thought.rawText;
}

export function timelineFromSessionSnapshot(
  session: AppInteractionSessionDescriptor | null | undefined,
): TimelineItem[] {
  return (session?.state.messages ?? []).flatMap(timelineItemsFromMessage);
}

function timelineItemsFromMessage(message: AppMessage): TimelineItem[] {
  const items: TimelineItem[] = [];
  for (const [index, block] of (message.content ?? []).entries()) {
    if (block.type !== "thinking") {
      continue;
    }
    const text = block.thinking.text ?? "";
    if (text.length === 0) {
      continue;
    }
    const kind = block.thinking.kind ?? "raw";
    items.push({
      kind: "reasoning",
      thoughtId: `${message.id}:thinking:${index}`,
      runId: message.id,
      status: "completed",
      summaryText: kind === "summary" ? text : "",
      rawText: kind === "summary" ? "" : text,
      expanded: false,
    });
  }

  const text = textFromMessage(message);
  if (text.length > 0) {
    items.push({
      kind: "message",
      id: message.id,
      role: message.role,
      text,
    });
  }
  return items;
}

function convergeTimelineToSessionSnapshot(
  current: TimelineItem[],
  session: AppInteractionSessionDescriptor,
): TimelineItem[] {
  const snapshotItems = timelineFromSessionSnapshot(session);
  const used = new Set<number>();
  const next = current.flatMap((item) => {
    const replacementIndex = replacementSnapshotItemIndex(item, snapshotItems, used);
    if (replacementIndex === -1) {
      return shouldDropUnmatchedCurrentItem(item) ? [] : [item];
    }
    used.add(replacementIndex);
    return [mergeTimelineItemWithSnapshot(item, snapshotItems[replacementIndex])];
  });

  for (const [index, item] of snapshotItems.entries()) {
    if (!used.has(index)) {
      next.push(item);
    }
  }
  return next;
}

function replacementSnapshotItemIndex(
  item: TimelineItem,
  snapshotItems: TimelineItem[],
  used: Set<number>,
): number {
  return snapshotItems.findIndex((candidate, index) => {
    if (used.has(index) || candidate.kind !== item.kind) {
      return false;
    }
    switch (item.kind) {
      case "message":
        return (
          candidate.kind === "message" &&
          candidate.role === item.role &&
          (candidate.id === item.id || candidate.text === item.text || item.live || item.pending)
        );
      case "reasoning":
        return candidate.kind === "reasoning" && sameReasoningSnapshot(item, candidate);
      case "tool":
      case "approval":
        return false;
    }
  });
}

function shouldDropUnmatchedCurrentItem(item: TimelineItem): boolean {
  return item.kind === "message" && Boolean(item.live || item.pending);
}

function mergeTimelineItemWithSnapshot(
  current: TimelineItem,
  snapshot: TimelineItem,
): TimelineItem {
  if (current.kind === "reasoning" && snapshot.kind === "reasoning") {
    return {
      ...snapshot,
      thoughtId: current.thoughtId,
      runId: current.runId || snapshot.runId,
      elapsedMs: current.elapsedMs ?? snapshot.elapsedMs,
      expanded: current.expanded,
    };
  }
  return snapshot;
}

function sameReasoningSnapshot(
  current: ReasoningTimelineItem,
  snapshot: ReasoningTimelineItem,
): boolean {
  return (
    current.thoughtId === snapshot.thoughtId ||
    (current.summaryText.length > 0 && current.summaryText === snapshot.summaryText) ||
    (current.rawText.length > 0 && current.rawText === snapshot.rawText) ||
    (reasoningVisibleText(current).length > 0 &&
      reasoningVisibleText(current) === reasoningVisibleText(snapshot))
  );
}

function sessionStatusToRunStatus(
  status: AppInteractionSessionDescriptor["status"] | null | undefined,
): RunStatus {
  switch (status) {
    case "running":
    case "paused":
    case "failed":
    case "aborted":
    case "completed":
      return status;
    case "idle":
    case undefined:
    case null:
      return "idle";
  }
}

function isTerminalRunStatus(status: RunStatus): boolean {
  return status === "completed" || status === "failed" || status === "aborted";
}

function defaultReasoning(thoughtId: string): ReasoningTimelineItem {
  return {
    kind: "reasoning",
    thoughtId,
    runId: "",
    status: "running",
    summaryText: "",
    rawText: "",
    expanded: false,
  };
}

function completeReasoning(
  timeline: TimelineItem[],
  thoughtId: string,
  runId: string,
  elapsedMs: number,
): TimelineItem[] {
  const completed = upsertReasoning(timeline, thoughtId, (thought) => ({
    ...thought,
    runId,
    status: "completed",
    elapsedMs,
  }));
  return completed.filter(
    (item) => item.kind !== "reasoning" || item.thoughtId !== thoughtId || hasReasoningContent(item),
  );
}

function settleRunReasoning(timeline: TimelineItem[], runId: string): TimelineItem[] {
  return timeline.flatMap((item) => {
    if (item.kind !== "reasoning" || item.runId !== runId || item.status !== "running") {
      return [item];
    }
    if (!hasReasoningContent(item)) {
      return [];
    }
    return [{ ...item, status: "completed" }];
  });
}

function hasReasoningContent(thought: ReasoningTimelineItem): boolean {
  return reasoningVisibleText(thought).trim().length > 0;
}

function upsertReasoning(
  timeline: TimelineItem[],
  thoughtId: string,
  update: (thought: ReasoningTimelineItem) => ReasoningTimelineItem,
): TimelineItem[] {
  const index = timeline.findIndex((item) => item.kind === "reasoning" && item.thoughtId === thoughtId);
  if (index === -1) {
    return [...timeline, update(defaultReasoning(thoughtId))];
  }
  return timeline.map((item, itemIndex) =>
    itemIndex === index && item.kind === "reasoning" ? update(item) : item,
  );
}

function defaultTool(toolCallId: string): ToolTimelineItem {
  return {
    kind: "tool",
    toolCallId,
    toolName: "tool",
    status: "running",
    updates: [],
    outputText: "",
    isError: false,
  };
}

function upsertTool(
  timeline: TimelineItem[],
  toolCallId: string,
  update: (tool: ToolTimelineItem) => ToolTimelineItem,
): TimelineItem[] {
  const index = timeline.findIndex((item) => item.kind === "tool" && item.toolCallId === toolCallId);
  if (index === -1) {
    return [...timeline, update(defaultTool(toolCallId))];
  }
  return timeline.map((item, itemIndex) =>
    itemIndex === index && item.kind === "tool" ? update(item) : item,
  );
}

function upsertApprovalRequest(
  timeline: TimelineItem[],
  approval: AppToolApprovalRequest,
): TimelineItem[] {
  const next = approvalFromRequest(approval);
  const index = timeline.findIndex(
    (item) => item.kind === "approval" && item.approvalId === approval.approvalId,
  );
  if (index === -1) {
    return [...timeline, next];
  }
  return timeline.map((item, itemIndex) =>
    itemIndex === index && item.kind === "approval"
      ? { ...next, status: item.status }
      : item,
  );
}

function updateApprovalStatus(
  timeline: TimelineItem[],
  approvalId: string,
  status: ApprovalTimelineItem["status"],
): TimelineItem[] {
  return timeline.map((item) =>
    item.kind === "approval" && item.approvalId === approvalId ? { ...item, status } : item,
  );
}

function approvalFromRequest(approval: AppToolApprovalRequest): ApprovalTimelineItem {
  const details = approvalRequestDetails(approval);
  return {
    kind: "approval",
    approvalId: approval.approvalId,
    toolCallId: approval.toolCall.id,
    toolName: approval.toolCall.name,
    prompt: approval.request.prompt ?? "",
    reason: approval.request.reason ?? "",
    command: details.command,
    cwd: details.cwd,
    permissionDescriptions: (approval.permissions ?? []).map(
      (permission) => permission.description ?? permission.capability,
    ),
    status: "pending",
  };
}

function approvalRequestDetails(approval: AppToolApprovalRequest): {
  command: string | null;
  cwd: string | null;
} {
  const metadata = recordValue(approval.request.metadata);
  const args = recordValue(approval.toolCall.arguments);
  return {
    command: stringValue(metadata?.command) ?? stringValue(args?.command),
    cwd: stringValue(metadata?.cwd) ?? stringValue(args?.cwd),
  };
}

function recordValue(value: unknown): Record<string, unknown> | null {
  return value != null && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function stringValue(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function appendAssistantDelta(
  timeline: TimelineItem[],
  displayMessageId: string,
  text: string,
): TimelineItem[] {
  if (text.length === 0) {
    return timeline;
  }
  const index = timeline.findIndex((item) => item.kind === "message" && item.id === displayMessageId);
  if (index === -1) {
    return [
      ...timeline,
      {
        kind: "message",
        id: displayMessageId,
        role: "assistant",
        text,
        live: true,
      },
    ];
  }
  return timeline.map((item, itemIndex) =>
    itemIndex === index && item.kind === "message"
      ? {
          ...item,
          text: `${item.text}${text}`,
          live: true,
        }
      : item,
  );
}

function applyAssistantFinal(
  timeline: TimelineItem[],
  displayMessageId: string,
  message: AppMessage,
): TimelineItem[] {
  const finalItems = timelineItemsFromMessage(message);
  const finalMessage = finalItems.find(
    (item): item is MessageTimelineItem => item.kind === "message",
  );
  const reasoningItems = finalItems.filter((item) => item.kind === "reasoning");

  const withoutLive = timeline.filter(
    (item) => item.kind !== "message" || (item.id !== displayMessageId && item.id !== message.id),
  );
  if (!finalMessage) {
    return [...withoutLive, ...reasoningItems];
  }
  return [...withoutLive, ...reasoningItems, finalMessage];
}

function approvalStatusFromDecision(
  decision: AppToolPermissionDecision,
): ApprovalTimelineItem["status"] {
  return decision.outcome === "allow" ? "approved" : "denied";
}

function textFromToolUpdate(update: AppToolUpdate): string {
  return textFromContentBlocks(update.content ?? []) || readableJson(update.details);
}

function textFromToolOutput(output: AppToolOutput): string {
  return textFromContentBlocks(output.content ?? []) || readableJson(output.details);
}
