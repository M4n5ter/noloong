// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import { resetDevInteractionRuntimeForTests } from "./devInteractionRuntime";
import {
  completedSessionWithText,
  emptySession,
  FakeInteractionRuntime,
} from "./test/fakeInteractionRuntime";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

const tauriEvent = vi.hoisted(() => ({
  emitTo: vi.fn(),
  listen: vi.fn(),
}));

const tauriCore = vi.hoisted(() => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: tauriCore.invoke,
}));

vi.mock("@tauri-apps/api/event", () => ({
  emitTo: tauriEvent.emitTo,
  listen: tauriEvent.listen,
}));

describe("transcript accessibility", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/");
    tauriEvent.emitTo.mockReset();
    tauriEvent.listen.mockReset();
    tauriCore.invoke.mockReset();
    tauriCore.invoke.mockResolvedValue(undefined);
    tauriEvent.listen.mockResolvedValue(() => {});
    resetDevInteractionRuntimeForTests();
    let frameTime = 0;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frameTime += 16;
      callback(frameTime);
      return 1;
    });
  });

  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("keeps completed assistant messages attributed without visible role labels", async () => {
    const runtime = new FakeInteractionRuntime(completedSessionWithText("final assistant response"));

    render(<App dependencies={dependenciesFor(runtime)} />);

    const message = await screen.findByRole("article", { name: "Assistant message" });
    expect(message).toHaveTextContent("final assistant response");
    expect(screen.queryByText("assistant")).not.toBeInTheDocument();
  });

  it("announces a local prompt as sending until the assistant starts responding", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await user.type(await screen.findByPlaceholderText("Write a message..."), "hello from user");
    await user.click(screen.getByRole("button", { name: "Send message" }));

    const pendingMessage = screen.getByRole("article", { name: "Sending your message" });
    expect(pendingMessage).toHaveAttribute("aria-busy", "true");
    expect(pendingMessage).toHaveTextContent("hello from user");
    expect(pendingMessage).not.toHaveTextContent("sending");

    act(() => {
      runtime.emitAssistantDelta("assistant response");
    });

    const userMessage = await screen.findByRole("article", { name: "Your message" });
    expect(userMessage).toHaveTextContent("hello from user");
    expect(screen.queryByRole("article", { name: "Sending your message" })).not.toBeInTheDocument();
  });

  it("keeps standalone tool details hidden until the activity row is expanded", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");

    act(() => {
      runtime.emitDisplayEvent({
        type: "tool_started",
        toolCallId: "tool-1",
        toolName: "desktop.preview.inspect",
      });
      runtime.emitDisplayEvent({
        type: "tool_updated",
        toolCallId: "tool-1",
        update: { content: [{ type: "text", text: "Captured viewport metrics." }] },
      });
      runtime.emitDisplayEvent({
        type: "tool_completed",
        toolCallId: "tool-1",
        output: { content: [{ type: "text", text: "Viewport check complete." }] },
      });
    });

    const activity = toolActivity("Inspecting preview");

    expect(screen.getByText("Done")).toBeVisible();
    expect(screen.getByText("Viewport check complete.")).toBeVisible();
    expect(screen.getByText("desktop.preview.inspect")).not.toBeVisible();
    expect(toolAuditDetail(activity)).not.toBeVisible();

    await user.click(screen.getByText("Inspecting preview"));

    expect(screen.getByText("desktop.preview.inspect")).toBeVisible();
    expect(toolAuditDetail(activity)).toBeVisible();
  });

  it("keeps completed reasoning tools low-noise until expanded", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");

    emitReasoningSummary(runtime);
    act(() => {
      runtime.emitDisplayEvent({
        type: "tool_started",
        toolCallId: "tool-1",
        toolName: "desktop.preview.inspect",
      });
      runtime.emitDisplayEvent({
        type: "tool_updated",
        toolCallId: "tool-1",
        update: { content: [{ type: "text", text: "Captured viewport metrics." }] },
      });
      runtime.emitDisplayEvent({
        type: "tool_completed",
        toolCallId: "tool-1",
        output: { content: [{ type: "text", text: "Viewport check complete." }] },
      });
      runtime.emitDisplayEvent({
        type: "thought_completed",
        runId: "run-1",
        thoughtId: "thought-1",
        elapsedMs: 360,
      });
    });

    await waitFor(() => expect(screen.queryByText("Thinking")).not.toBeInTheDocument());

    const activity = toolActivity("Inspecting preview");
    const summary = toolActivitySummary(activity);
    expect(summary).not.toHaveTextContent("Done");
    expect(summary).not.toHaveTextContent("Viewport check complete.");
    expect(summary).toHaveAccessibleName("Inspecting preview, Done");

    const auditDetail = toolAuditDetail(activity);
    expect(auditDetail).not.toBeVisible();
    expect(auditDetail).toHaveTextContent("Viewport check complete.");

    await user.click(screen.getByText("Inspecting preview"));

    expect(auditDetail).toBeVisible();
    expect(auditDetail).toHaveTextContent("Captured viewport metrics.");
  });

  it("keeps tools started after completed reasoning explicit", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");

    emitReasoningSummary(runtime);
    act(() => {
      runtime.emitDisplayEvent({
        type: "thought_completed",
        runId: "run-1",
        thoughtId: "thought-1",
        elapsedMs: 360,
      });
      runtime.emitDisplayEvent({
        type: "tool_started",
        toolCallId: "tool-1",
        toolName: "desktop.preview.inspect",
      });
      runtime.emitDisplayEvent({
        type: "tool_completed",
        toolCallId: "tool-1",
        output: { content: [{ type: "text", text: "Viewport check complete." }] },
      });
    });

    const summary = toolActivitySummary(toolActivity("Inspecting preview"));
    expect(within(summary).getByText("Done")).toBeVisible();
    expect(within(summary).getByText("Viewport check complete.")).toBeVisible();
    expect(summary).not.toHaveAccessibleName("Inspecting preview, Done");
  });

  it("keeps failed reasoning tools explicit", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");

    emitReasoningSummary(runtime);
    act(() => {
      runtime.emitDisplayEvent({
        type: "tool_started",
        toolCallId: "tool-1",
        toolName: "host.exec.start",
      });
      runtime.emitDisplayEvent({
        type: "tool_completed",
        toolCallId: "tool-1",
        output: { content: [{ type: "text", text: "Permission denied." }], isError: true },
      });
    });

    const summary = toolActivitySummary(toolActivity("Local command"));
    expect(within(summary).getByText("Failed")).toBeVisible();
    expect(within(summary).getByText("Permission denied.")).toBeVisible();
  });
});

function dependenciesFor(runtime: FakeInteractionRuntime, locale: "en" | "zh" = "en") {
  return {
    bootstrap: async () => runtime.bootstrap(locale),
    createInteractionClient: runtime.createClient,
    connectInteractionDisplayStream: runtime.connectDisplayStream,
  };
}

function emitReasoningSummary(runtime: FakeInteractionRuntime): void {
  act(() => {
    runtime.emitDisplayEvent({
      type: "thought_started",
      runId: "run-1",
      thoughtId: "thought-1",
    });
    runtime.emitDisplayEvent({
      type: "thought_delta",
      runId: "run-1",
      thoughtId: "thought-1",
      kind: "summary",
      text: "Reading the current UI state.",
    });
  });
}

function toolActivity(title: string): HTMLElement {
  const summary = toolActivitySummaryFromTitle(title);
  const activity = summary.closest("details");
  if (!(activity instanceof HTMLElement)) {
    throw new Error(`Tool activity was not rendered for ${title}`);
  }
  return activity;
}

function toolActivitySummary(activity: HTMLElement): HTMLElement {
  const summary = activity.querySelector("summary");
  if (!(summary instanceof HTMLElement)) {
    throw new Error("Tool activity summary was not rendered");
  }
  return summary;
}

function toolActivitySummaryFromTitle(title: string): HTMLElement {
  const titleElement = screen.getByText(title);
  const summary = titleElement.closest("summary");
  if (!(summary instanceof HTMLElement)) {
    throw new Error(`Tool activity summary was not rendered for ${title}`);
  }
  return summary;
}

function toolAuditDetail(activity: HTMLElement): HTMLElement {
  const detail = activity.querySelector(".tool-activity-audit pre");
  if (!(detail instanceof HTMLElement)) {
    throw new Error("Tool audit detail was not rendered");
  }
  return detail;
}
