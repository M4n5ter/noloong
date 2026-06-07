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
    let frameTime = 0;
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      frameTime += 16;
      callback(frameTime);
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

  it("declares native Tauri title bar drag regions", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    expect(await screen.findByText("fake-interaction · test")).toBeInTheDocument();

    expect(document.querySelector(".title-bar")).toHaveAttribute(
      "data-tauri-drag-region",
      "deep",
    );
    expect(screen.getByRole("button", { name: "Chat" })).toHaveAttribute(
      "data-tauri-drag-region",
      "false",
    );
    expect(screen.getByRole("button", { name: "Settings" })).toHaveAttribute(
      "data-tauri-drag-region",
      "false",
    );
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
    await expectVisibleText("first");
    expect(document.body).not.toHaveTextContent("first second");

    act(() => runtime.emitAssistantDelta(" second"));
    await expectVisibleText("first second");
  });

  it("converges to the authoritative session snapshot after run completion", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");
    await displayStreamReady();
    act(() => runtime.emitAssistantDelta("draft answer"));
    await expectVisibleText("draft answer");

    runtime.setSession(completedSessionWithText("final answer"));
    act(() => runtime.emitRunCompleted());

    await waitFor(() => expect(screen.getAllByText("final answer").length).toBeGreaterThan(0));
    expect(document.body).not.toHaveTextContent("draft answer");
  });

  it("does not let a stale prompt response overwrite terminal snapshot convergence", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByPlaceholderText("Write a message...");
    await user.type(composer, "stream please");
    await user.click(screen.getByRole("button", { name: "↑" }));

    act(() => runtime.emitAssistantDelta("draft answer"));
    await expectVisibleText("draft answer");

    runtime.setSession(completedSessionWithText("final answer"));
    act(() => runtime.emitRunCompleted());
    await waitFor(() => expect(screen.getAllByText("final answer").length).toBeGreaterThan(0));

    act(() => runtime.resolvePrompt(completedSessionWithText("stale prompt answer")));

    await waitFor(() => {
      expect(screen.getAllByText("final answer").length).toBeGreaterThan(0);
      expect(document.body).not.toHaveTextContent("draft answer");
      expect(document.body).not.toHaveTextContent("stale prompt answer");
    });
  });

  it("settles to the final snapshot when terminal state is visible before the committed message", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");
    await displayStreamReady();
    act(() => runtime.emitAssistantDelta("draft answer"));
    await expectVisibleText("draft answer");

    runtime.queueGetSessionResponse(emptySession());
    act(() => runtime.emitRunCompleted());
    runtime.setSession(completedSessionWithText("final answer"));

    await waitFor(
      () => {
        expect(screen.getAllByText("final answer").length).toBeGreaterThan(0);
        expect(document.body).not.toHaveTextContent("draft answer");
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

  it("keeps following output when content growth fires a passive scroll event", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    const transcript = await transcriptElement();
    setScrollMetrics(transcript, { scrollHeight: 1000, clientHeight: 300 });
    transcript.scrollTop = 690;
    fireEvent.scroll(transcript);

    setScrollMetrics(transcript, { scrollHeight: 1200, clientHeight: 300 });
    fireEvent.scroll(transcript);

    act(() => runtime.emitAssistantDelta("passive-growth-follow"));

    await waitFor(() => expect(transcript.scrollTop).toBe(1200));
  });

  it("keeps following output when streamed markdown growth resizes the transcript content", async () => {
    let resizeObserverCallback: ResizeObserverCallback | undefined;
    class MockResizeObserver {
      constructor(callback: ResizeObserverCallback) {
        resizeObserverCallback = callback;
      }

      observe = vi.fn();
      unobserve = vi.fn();
      disconnect = vi.fn();
    }
    vi.stubGlobal("ResizeObserver", MockResizeObserver);

    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    const transcript = await transcriptElement();
    await waitFor(() => expect(resizeObserverCallback).toBeDefined());
    setScrollMetrics(transcript, { scrollHeight: 1000, clientHeight: 300 });
    transcript.scrollTop = 690;
    fireEvent.scroll(transcript);

    setScrollMetrics(transcript, { scrollHeight: 1400, clientHeight: 300 });
    act(() => {
      resizeObserverCallback?.([], {} as ResizeObserver);
    });

    await waitFor(() => expect(transcript.scrollTop).toBe(1400));
  });

  it("does not force the transcript to the bottom after the user scrolls up", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    const transcript = await transcriptElement();
    setScrollMetrics(transcript, { scrollHeight: 1000, clientHeight: 300 });
    transcript.scrollTop = 690;
    fireEvent.scroll(transcript);
    transcript.scrollTop = 100;
    fireEvent.scroll(transcript);

    act(() => runtime.emitAssistantDelta("keep-position"));

    await expectVisibleText("keep-position");
    expect(transcript.scrollTop).toBe(100);
  });

  it("resumes bottom-following after the user sends a new prompt", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const transcript = await transcriptElement();
    setScrollMetrics(transcript, { scrollHeight: 1000, clientHeight: 300 });
    transcript.scrollTop = 690;
    fireEvent.scroll(transcript);
    transcript.scrollTop = 100;
    fireEvent.scroll(transcript);

    const composer = await screen.findByPlaceholderText("Write a message...");
    await user.type(composer, "follow again");
    await user.click(screen.getByRole("button", { name: "↑" }));

    act(() => runtime.emitAssistantDelta("after-send-follow"));

    await waitFor(() => expect(transcript.scrollTop).toBe(1000));
  });

  it("renders live reasoning markdown and folds it after completion", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");
    await displayStreamReady();

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
        text: "## Reasoning Summary\n\n- markdown item",
      });
    });

    await expectVisibleText("Reasoning Summary");
    await expectVisibleText("markdown item");

    act(() => {
      runtime.emitDisplayEvent({
        type: "thought_completed",
        runId: "run-1",
        thoughtId: "thought-1",
        elapsedMs: 2000,
      });
    });

    await waitFor(() => expect(screen.getByText("Thought for 2 seconds")).toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Show raw" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Reasoning Summary" })).not.toBeInTheDocument();
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
  await displayStreamReady();
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

async function displayStreamReady(): Promise<void> {
  await screen.findByText("Display stream ready");
}

async function expectVisibleText(text: string): Promise<void> {
  await waitFor(() => expect(document.body).toHaveTextContent(text));
}
