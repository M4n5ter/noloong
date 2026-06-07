// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import {
  completedSessionWithText,
  emptySession,
  FakeInteractionRuntime,
} from "./test/fakeInteractionRuntime";

describe("Noloong app chat regression harness", () => {
  beforeEach(() => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(0);
      return 1;
    });
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("starts a fake interaction runtime and shows the loaded session", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    expect(await screen.findByText("fake-interaction · test")).toBeInTheDocument();
    expect(screen.getAllByText("default · idle").length).toBeGreaterThan(0);
  });

  it("shows the local user message immediately after sending", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByPlaceholderText("Write a message...");
    await user.type(composer, "hello from user");
    await user.click(screen.getByRole("button", { name: "↑" }));

    expect(screen.getByText("hello from user")).toBeInTheDocument();
    expect(runtime.promptRequests[0]).toMatchObject({
      sessionId: "session-1",
      input: { type: "text", text: "hello from user" },
    });
  });

  it("renders display deltas in separate batches before the final snapshot arrives", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByPlaceholderText("Write a message...");
    await user.type(composer, "stream please");
    await user.click(screen.getByRole("button", { name: "↑" }));

    act(() => runtime.emitAssistantDelta("first"));
    expect(await screen.findByText("first")).toBeInTheDocument();
    expect(screen.queryByText("first second")).not.toBeInTheDocument();

    act(() => runtime.emitAssistantDelta(" second"));
    expect(await screen.findByText("first second")).toBeInTheDocument();
  });

  it("converges to the authoritative session snapshot after run completion", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");
    act(() => runtime.emitAssistantDelta("draft answer"));
    expect(await screen.findByText("draft answer")).toBeInTheDocument();

    runtime.setSession(completedSessionWithText("final answer"));
    act(() => runtime.emitRunCompleted());

    await waitFor(() => expect(screen.getAllByText("final answer").length).toBeGreaterThan(0));
    expect(screen.queryByText("draft answer")).not.toBeInTheDocument();
  });

  it("does not let a stale prompt response overwrite terminal snapshot convergence", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByPlaceholderText("Write a message...");
    await user.type(composer, "stream please");
    await user.click(screen.getByRole("button", { name: "↑" }));

    act(() => runtime.emitAssistantDelta("draft answer"));
    expect(await screen.findByText("draft answer")).toBeInTheDocument();

    runtime.setSession(completedSessionWithText("final answer"));
    act(() => runtime.emitRunCompleted());
    await waitFor(() => expect(screen.getAllByText("final answer").length).toBeGreaterThan(0));

    act(() => runtime.resolvePrompt(completedSessionWithText("stale prompt answer")));

    await waitFor(() => {
      expect(screen.getAllByText("final answer").length).toBeGreaterThan(0);
      expect(screen.queryByText("draft answer")).not.toBeInTheDocument();
      expect(screen.queryByText("stale prompt answer")).not.toBeInTheDocument();
    });
  });

  it("settles to the final snapshot when terminal state is visible before the committed message", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");
    act(() => runtime.emitAssistantDelta("draft answer"));
    expect(await screen.findByText("draft answer")).toBeInTheDocument();

    runtime.queueGetSessionResponse(emptySession());
    act(() => runtime.emitRunCompleted());
    runtime.setSession(completedSessionWithText("final answer"));

    await waitFor(
      () => {
        expect(screen.getAllByText("final answer").length).toBeGreaterThan(0);
        expect(screen.queryByText("draft answer")).not.toBeInTheDocument();
      },
      { timeout: 1000 },
    );
  });

  it("follows new output when the transcript is near the bottom", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    const transcript = await transcriptElement();
    setScrollMetrics(transcript, { scrollHeight: 1000, clientHeight: 300 });
    transcript.scrollTop = 690;

    act(() => runtime.emitAssistantDelta("tail-follow"));

    await waitFor(() => expect(transcript.scrollTop).toBe(1000));
  });

  it("does not force the transcript to the bottom after the user scrolls up", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    const transcript = await transcriptElement();
    setScrollMetrics(transcript, { scrollHeight: 1000, clientHeight: 300 });
    transcript.scrollTop = 100;
    fireEvent.scroll(transcript);

    act(() => runtime.emitAssistantDelta("keep-position"));

    await waitFor(() => expect(screen.getByText("keep-position")).toBeInTheDocument());
    expect(transcript.scrollTop).toBe(100);
  });
});

function dependenciesFor(runtime: FakeInteractionRuntime) {
  return {
    bootstrap: async () => runtime.bootstrap("en"),
    createInteractionClient: runtime.createClient,
    connectInteractionDisplayStream: runtime.connectDisplayStream,
  };
}

async function transcriptElement(): Promise<HTMLDivElement> {
  await screen.findByPlaceholderText("Write a message...");
  const transcript = document.querySelector(".transcript");
  if (!(transcript instanceof HTMLDivElement)) {
    throw new Error("transcript element was not rendered");
  }
  return transcript;
}

function setScrollMetrics(
  element: HTMLElement,
  metrics: { scrollHeight: number; clientHeight: number },
): void {
  Object.defineProperties(element, {
    scrollHeight: { configurable: true, value: metrics.scrollHeight },
    clientHeight: { configurable: true, value: metrics.clientHeight },
  });
}
