// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { open } from "@tauri-apps/plugin-dialog";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { App } from "./App";
import {
  completedSessionWithText,
  emptySession,
  FakeInteractionRuntime,
} from "./test/fakeInteractionRuntime";

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

describe("Noloong app chat regression harness", () => {
  beforeEach(() => {
    vi.mocked(open).mockReset();
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

    expect(await screen.findByRole("heading", { name: "New session" })).toBeInTheDocument();
    expect(screen.getAllByText("default · idle").length).toBeGreaterThan(0);
    expect(screen.queryByRole("button", { name: "Open settings" })).not.toBeInTheDocument();
  });

  it("opens settings with the macOS settings shortcut and returns to chat", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByRole("heading", { name: "New session" });
    await user.keyboard("{Meta>},{/Meta}");

    expect(await screen.findByRole("button", { name: "Back to chat" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Back to chat" }));

    expect(screen.getByRole("heading", { name: "New session" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open settings" })).not.toBeInTheDocument();
  });

  it("also accepts the control-comma settings shortcut fallback", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByRole("heading", { name: "New session" });
    await user.keyboard("{Control>},{/Control}");

    expect(await screen.findByRole("button", { name: "Back to chat" })).toBeInTheDocument();
  });

  it("opens an environment pane directly from Provider", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByRole("heading", { name: "New session" });
    await user.keyboard("{Meta>},{/Meta}");
    await screen.findByRole("button", { name: "Back to chat" });

    await user.click(screen.getByRole("button", { name: "Provider" }));

    expect(await screen.findByRole("heading", { name: "Provider" })).toBeInTheDocument();
  });

  it("keeps the desktop session toolbar icon-only with accessible names", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByRole("heading", { name: "New session" });
    const toolbar = screen.getByRole("complementary", { name: "Session controls" });

    expect(within(toolbar).getByRole("button", { name: "Sessions" })).toBeInTheDocument();
    expect(within(toolbar).getByRole("button", { name: "Create session" })).toBeInTheDocument();
    expect(within(toolbar).queryByRole("button", { name: "Open settings" })).not.toBeInTheDocument();

    await user.click(within(toolbar).getByRole("button", { name: "Sessions" }));

    expect(screen.getByRole("dialog", { name: "Sessions" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Return" })).toHaveFocus();
    expect(within(toolbar).queryByRole("button", { name: "Sessions" })).not.toBeInTheDocument();
  });

  it("opens the sessions panel as a modal and restores focus when it closes", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const sessionsButton = await screen.findByRole("button", { name: "Sessions" });
    await user.click(sessionsButton);

    const dialog = screen.getByRole("dialog", { name: "Sessions" });
    expect(dialog).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Return" })).toHaveFocus();

    await user.keyboard("{Escape}");

    expect(screen.queryByRole("dialog", { name: "Sessions" })).not.toBeInTheDocument();
    expect(sessionsButton).toHaveFocus();
  });

  it("does not leave the chat inert if the sessions panel action fails", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();
    runtime.failNextCreateSession();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await user.click(await screen.findByRole("button", { name: "Sessions" }));
    expect(screen.getByRole("dialog", { name: "Sessions" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Create session" }));

    const failureStatus = await screen.findByRole("status");
    expect(failureStatus).toHaveTextContent("Interaction failed");
    expect(screen.queryByRole("dialog", { name: "Sessions" })).not.toBeInTheDocument();
    expect(within(failureStatus).getByRole("heading", { name: "Interaction failed" })).toBeVisible();
    expect(screen.getByRole("complementary", { name: "Session controls" })).toBeVisible();
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

  it("reveals the composer expander when measured text overflows the compact capsule", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByPlaceholderText("Write a message...");
    expect(screen.queryByRole("button", { name: "Expand composer" })).not.toBeInTheDocument();
    Object.defineProperties(composer, {
      clientHeight: { configurable: true, value: 38 },
      scrollHeight: { configurable: true, value: 64 },
      clientWidth: { configurable: true, value: 240 },
      scrollWidth: { configurable: true, value: 240 },
    });

    fireEvent.change(composer, { target: { value: "wrapped text" } });

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Expand composer" })).toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: "Expand composer" }));
    expect(screen.getByRole("button", { name: "Collapse composer" })).toBeInTheDocument();
  });

  it("sends attachment prompts as message content blocks", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();
    vi.mocked(open).mockResolvedValue(["/tmp/reference.png"]);

    render(<App dependencies={dependenciesFor(runtime)} />);

    await user.click(await screen.findByRole("button", { name: "Attach files" }));
    expect(await screen.findByText("reference.png")).toBeInTheDocument();

    const composer = await screen.findByPlaceholderText("Write a message...");
    await user.type(composer, "inspect this");
    await user.click(screen.getByRole("button", { name: "↑" }));

    await waitFor(() => expect(runtime.promptRequests).toHaveLength(1));
    expect(runtime.promptRequests[0]).toMatchObject({
      sessionId: "session-1",
      input: {
        type: "message",
        message: {
          role: "user",
          content: [
            { type: "text", text: "inspect this" },
            {
              type: "media",
              media: {
                kind: "image",
                source: { type: "uri", uri: "file:///tmp/reference.png" },
                mimeType: "image/png",
                name: "reference.png",
              },
            },
          ],
        },
      },
    });
  });

  it("removes attachments without submitting the prompt", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();
    vi.mocked(open).mockResolvedValue(["/tmp/reference.png"]);

    render(<App dependencies={dependenciesFor(runtime)} />);

    await user.click(await screen.findByRole("button", { name: "Attach files" }));
    expect(await screen.findByText("reference.png")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Remove reference.png" }));
    expect(screen.queryByText("reference.png")).not.toBeInTheDocument();
    expect(runtime.promptRequests).toHaveLength(0);

    const composer = await screen.findByPlaceholderText("Write a message...");
    await user.type(composer, "text only");
    await user.click(screen.getByRole("button", { name: "↑" }));

    await waitFor(() => expect(runtime.promptRequests).toHaveLength(1));
    expect(runtime.promptRequests[0]).toMatchObject({
      sessionId: "session-1",
      input: { type: "text", text: "text only" },
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
    await composerReadyForInput();
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
    await composerReadyForInput();
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

  it("renders live reasoning markdown and removes it from the reading flow after completion", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");
    await composerReadyForInput();

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

    await waitFor(() => expect(screen.queryByText("Thinking")).not.toBeInTheDocument());
    expect(screen.queryByRole("button", { name: "Show raw" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Reasoning Summary" })).not.toBeInTheDocument();
  });

  it("resolves approval requests from a localized decision card without showing raw protocol text", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");
    await composerReadyForInput();

    act(() => {
      runtime.emitDisplayEvent({
        type: "approval_requested",
        approval: {
          approvalId: "approval-1",
          toolCall: { id: "call-1", name: "host.exec.start" },
          request: {
            prompt: "Run command?",
            reason: "需要人工审批",
            metadata: {
              command: "pwd && ls -la",
              cwd: "/Users/m4n5ter/rust/noloong",
            },
          },
          permissions: [
            {
              capability: "host.exec",
              description: "Run host commands.",
            },
          ],
        },
      });
    });

    const card = await screen.findByRole("article", { name: "Approval required" });
    expect(within(card).getByText("pwd && ls -la")).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Allow" })).toBeInTheDocument();
    expect(within(card).getByRole("button", { name: "Deny" })).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("Run command?");
    expect(document.body).not.toHaveTextContent("pending");

    await user.click(within(card).getByRole("button", { name: "Allow" }));

    await waitFor(() =>
      expect(runtime.approvalResolveRequests).toEqual([
        expect.objectContaining({
          approvalId: "approval-1",
          decision: expect.objectContaining({ outcome: "allow" }),
          sessionId: "session-1",
        }),
      ]),
    );
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
  await composerReadyForInput();
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

async function composerReadyForInput(): Promise<void> {
  await screen.findByPlaceholderText("Write a message...");
}

async function expectVisibleText(text: string): Promise<void> {
  await waitFor(() => expect(document.body).toHaveTextContent(text));
}
