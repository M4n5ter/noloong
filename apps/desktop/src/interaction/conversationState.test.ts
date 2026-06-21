import { describe, expect, it } from "vitest";
import type { AppDisplayEvent } from "../generated/contracts";
import {
  applyDisplayEventToConversation,
  appendLocalPrompt,
  convergeConversationToSessionSnapshot,
  conversationFromSessionSnapshot,
  emptyConversationState,
  markApprovalResolved,
  reasoningVisibleText,
  setReasoningExpanded,
} from "./conversationState";

describe("conversation timeline reducer", () => {
  it("keeps reasoning in timeline and prefers summary while raw text stays expandable", () => {
    const events: AppDisplayEvent[] = [
      {
        type: "thought_started",
        runId: "run-1",
        thoughtId: "thought-1",
      },
      {
        type: "thought_delta",
        runId: "run-1",
        thoughtId: "thought-1",
        kind: "raw",
        text: "raw detail",
      },
      {
        type: "thought_delta",
        runId: "run-1",
        thoughtId: "thought-1",
        kind: "summary",
        text: "summary",
      },
      {
        type: "thought_completed",
        runId: "run-1",
        thoughtId: "thought-1",
        elapsedMs: 2100,
      },
    ];
    const state = events.reduce(applyDisplayEventToConversation, emptyConversationState());
    const thought = state.timeline[0];

    expect(thought).toMatchObject({
      kind: "reasoning",
      status: "completed",
      summaryText: "summary",
      rawText: "raw detail",
      elapsedMs: 2100,
      expanded: false,
    });
    if (thought?.kind !== "reasoning") {
      throw new Error("expected reasoning timeline item");
    }
    expect(reasoningVisibleText(thought)).toBe("summary");
    expect(setReasoningExpanded(state, "thought-1", true).timeline[0]).toMatchObject({
      expanded: true,
    });
  });

  it("drops empty completed reasoning placeholders", () => {
    const events: AppDisplayEvent[] = [
      {
        type: "thought_started",
        runId: "run-1",
        thoughtId: "thought-1",
      },
      {
        type: "thought_completed",
        runId: "run-1",
        thoughtId: "thought-1",
        elapsedMs: 1200,
      },
    ];

    const state = events.reduce(applyDisplayEventToConversation, emptyConversationState());

    expect(state.timeline).toEqual([]);
  });

  it("drops empty running reasoning when a run reaches a terminal state", () => {
    const events: AppDisplayEvent[] = [
      {
        type: "thought_started",
        runId: "run-1",
        thoughtId: "thought-1",
      },
      {
        type: "run_completed",
        runId: "run-1",
      },
    ];

    const state = events.reduce(applyDisplayEventToConversation, emptyConversationState());

    expect(state.timeline).toEqual([]);
  });

  it("tracks run status transitions", () => {
    const running = applyDisplayEventToConversation(emptyConversationState(), {
      type: "run_started",
      runId: "run-1",
    });
    expect(running.runStatus).toBe("running");

    const paused = applyDisplayEventToConversation(running, {
      type: "run_paused",
      runId: "run-1",
      reason: { type: "tool_approval" },
    });
    expect(paused.runStatus).toBe("paused");
    expect(paused.pauseReason).toContain("tool_approval");

    const failed = applyDisplayEventToConversation(paused, {
      type: "run_failed",
      runId: "run-1",
      error: "provider failed",
    });
    expect(failed.runStatus).toBe("failed");
    expect(failed.runError).toBe("provider failed");
  });

  it("keeps tool activity in the same timeline order as display events", () => {
    const events: AppDisplayEvent[] = [
      {
        type: "assistant_message_delta",
        runId: "run-1",
        displayMessageId: "display-1",
        text: "before tool",
      },
      {
        type: "tool_started",
        toolCallId: "call-1",
        toolName: "host.exec.start",
      },
      {
        type: "tool_updated",
        toolCallId: "call-1",
        update: {
          content: [{ type: "text", text: "running" }],
        },
      },
      {
        type: "tool_completed",
        toolCallId: "call-1",
        output: {
          content: [{ type: "text", text: "done" }],
          isError: false,
        },
      },
    ];
    const state = events.reduce(applyDisplayEventToConversation, emptyConversationState());

    expect(state.timeline.map((item) => item.kind)).toEqual(["message", "tool"]);
    expect(state.timeline[1]).toMatchObject({
      kind: "tool",
      toolCallId: "call-1",
      toolName: "host.exec.start",
      status: "completed",
      updates: ["running"],
      outputText: "done",
      isError: false,
    });
  });

  it("tracks approval lifecycle from display events and supports optimistic local resolution", () => {
    const requested = applyDisplayEventToConversation(emptyConversationState(), {
      type: "approval_requested",
      approval: {
        approvalId: "approval-1",
        toolCall: { id: "call-1", name: "host.exec.start" },
        request: {
          prompt: "Run command?",
          reason: "Needs host access.",
          metadata: {
            command: "pwd && ls -la",
            cwd: "/Users/m4n5ter/rust/noloong",
            targetPaths: ["apps/desktop/src/App.tsx", ""],
          },
        },
        permissions: [
          {
            capability: "host.command",
            description: "Run shell commands.",
          },
        ],
      },
    });

    expect(requested.timeline).toEqual([
      {
        kind: "approval",
        approvalId: "approval-1",
        toolCallId: "call-1",
        toolName: "host.exec.start",
        prompt: "Run command?",
        reason: "Needs host access.",
        command: "pwd && ls -la",
        cwd: "/Users/m4n5ter/rust/noloong",
        targetPaths: ["apps/desktop/src/App.tsx"],
        permissions: [
          {
            capability: "host.command",
            description: "Run shell commands.",
          },
        ],
        status: "pending",
      },
    ]);
    expect(markApprovalResolved(requested, "approval-1", "allow").timeline[0]).toMatchObject({
      status: "approved",
    });

    const resolved = applyDisplayEventToConversation(requested, {
      type: "approval_resolved",
      approvalId: "approval-1",
      decision: { outcome: "deny", approver: "test" },
    });
    expect(resolved.timeline[0]).toMatchObject({ status: "denied" });
  });

  it("hydrates thinking blocks as reasoning instead of assistant visible text", () => {
    const state = conversationFromSessionSnapshot({
      sessionId: "session-1",
      profileId: "default",
      status: "completed",
      state: {
        messages: [
          {
            id: "assistant-1",
            role: "assistant",
            content: [
              { type: "thinking", thinking: { kind: "summary", text: "private summary" } },
              { type: "text", text: "visible answer" },
            ],
          },
        ],
      },
    });

    expect(state.timeline).toEqual([
      {
        kind: "reasoning",
        thoughtId: "assistant-1:thinking:0",
        runId: "assistant-1",
        status: "completed",
        summaryText: "private summary",
        rawText: "",
        expanded: false,
      },
      {
        kind: "message",
        id: "assistant-1",
        role: "assistant",
        text: "visible answer",
      },
    ]);
  });

  it("converges live messages to the authoritative snapshot without dropping activity items", () => {
    const withPrompt = appendLocalPrompt(emptyConversationState(), "hello", "local-user-1");
    const withApproval = applyDisplayEventToConversation(withPrompt, {
      type: "approval_requested",
      approval: {
        approvalId: "approval-1",
        toolCall: { id: "call-1", name: "host.exec.start" },
        request: {},
      },
    });
    const live = applyDisplayEventToConversation(withApproval, {
      type: "assistant_message_delta",
      runId: "run-1",
      displayMessageId: "display-1",
      text: "draft",
    });

    const converged = convergeConversationToSessionSnapshot(live, {
      sessionId: "session-1",
      profileId: "default",
      status: "completed",
      state: {
        messages: [
          {
            id: "user-1",
            role: "user",
            content: [{ type: "text", text: "hello" }],
          },
          {
            id: "assistant-1",
            role: "assistant",
            content: [{ type: "text", text: "final" }],
          },
        ],
      },
    });

    expect(converged.runStatus).toBe("completed");
    expect(converged.timeline.map((item) => item.kind)).toEqual([
      "message",
      "approval",
      "message",
    ]);
    expect(converged.timeline[0]).toMatchObject({ id: "user-1", text: "hello" });
    expect(converged.timeline[2]).toMatchObject({ id: "assistant-1", text: "final" });
  });

  it("acknowledges optimistic user prompts once the assistant starts responding", () => {
    const withPrompt = appendLocalPrompt(emptyConversationState(), "hello", "local-user-1");

    const responding = applyDisplayEventToConversation(withPrompt, {
      type: "assistant_message_delta",
      runId: "run-1",
      displayMessageId: "display-1",
      text: "draft",
    });

    expect(responding.timeline[0]).toEqual({
      kind: "message",
      id: "local-user-1",
      role: "user",
      text: "hello",
      pending: true,
      acknowledged: true,
    });
    expect(responding.timeline[1]).toMatchObject({
      kind: "message",
      role: "assistant",
      text: "draft",
      live: true,
    });
  });

  it("keeps optimistic user prompts sending when an assistant delta is empty", () => {
    const withPrompt = appendLocalPrompt(emptyConversationState(), "hello", "local-user-1");

    const unchanged = applyDisplayEventToConversation(withPrompt, {
      type: "assistant_message_delta",
      runId: "run-1",
      displayMessageId: "display-1",
      text: "",
    });

    expect(unchanged.timeline).toEqual(withPrompt.timeline);
  });

  it("keeps acknowledged optimistic prompts mergeable when snapshot text differs", () => {
    const withPrompt = appendLocalPrompt(
      emptyConversationState(),
      "inspect this\n\n@reference.png",
      "local-user-1",
    );
    const live = applyDisplayEventToConversation(withPrompt, {
      type: "assistant_message_delta",
      runId: "run-1",
      displayMessageId: "display-1",
      text: "draft",
    });

    const converged = convergeConversationToSessionSnapshot(live, {
      sessionId: "session-1",
      profileId: "default",
      status: "completed",
      state: {
        messages: [
          {
            id: "user-1",
            role: "user",
            content: [{ type: "text", text: "inspect this" }],
          },
          {
            id: "assistant-1",
            role: "assistant",
            content: [{ type: "text", text: "final" }],
          },
        ],
      },
    });

    expect(converged.timeline).toHaveLength(2);
    expect(converged.timeline[0]).toMatchObject({
      kind: "message",
      id: "user-1",
      role: "user",
      text: "inspect this",
    });
    expect(converged.timeline[1]).toMatchObject({
      kind: "message",
      id: "assistant-1",
      role: "assistant",
      text: "final",
    });
  });

  it("converges snapshot thinking into existing live reasoning without duplicating it", () => {
    const events: AppDisplayEvent[] = [
      {
        type: "thought_started",
        runId: "run-1",
        thoughtId: "thought-live",
      },
      {
        type: "thought_delta",
        runId: "run-1",
        thoughtId: "thought-live",
        kind: "summary",
        text: "reasoning summary",
      },
      {
        type: "thought_completed",
        runId: "run-1",
        thoughtId: "thought-live",
        elapsedMs: 2000,
      },
    ];
    const liveReasoning = events.reduce(applyDisplayEventToConversation, emptyConversationState());

    const snapshot = {
      sessionId: "session-1",
      profileId: "default",
      status: "completed" as const,
      state: {
        messages: [
          {
            id: "assistant-1",
            role: "assistant",
            content: [
              { type: "thinking" as const, thinking: { kind: "summary", text: "reasoning summary" } },
              { type: "text" as const, text: "final answer" },
            ],
          },
        ],
      },
    };

    const once = convergeConversationToSessionSnapshot(liveReasoning, snapshot);
    const twice = convergeConversationToSessionSnapshot(once, snapshot);

    expect(twice.timeline.map((item) => item.kind)).toEqual(["reasoning", "message"]);
    expect(twice.timeline[0]).toMatchObject({
      kind: "reasoning",
      thoughtId: "thought-live",
      summaryText: "reasoning summary",
      elapsedMs: 2000,
    });
    expect(twice.timeline[1]).toMatchObject({ text: "final answer" });
  });

  it("ignores stale assistant deltas after a terminal run event", () => {
    const running = applyDisplayEventToConversation(emptyConversationState(), {
      type: "run_started",
      runId: "run-1",
    });
    const live = applyDisplayEventToConversation(running, {
      type: "assistant_message_delta",
      runId: "run-1",
      displayMessageId: "display-1",
      text: "draft",
    });
    const completed = applyDisplayEventToConversation(live, {
      type: "run_completed",
      runId: "run-1",
    });

    const stale = applyDisplayEventToConversation(completed, {
      type: "assistant_message_delta",
      runId: "run-1",
      displayMessageId: "display-1",
      text: " stale",
    });

    expect(stale.timeline).toEqual([
      {
        kind: "message",
        id: "display-1",
        role: "assistant",
        text: "draft",
        live: true,
      },
    ]);
  });
});
