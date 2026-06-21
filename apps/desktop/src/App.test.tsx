// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { open } from "@tauri-apps/plugin-dialog";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AppRuntimeRestartResult } from "./generated/contracts";
import { App } from "./App";
import {
  observeDevInteractionRuntimeForTests,
  resetDevInteractionRuntimeForTests,
} from "./devInteractionRuntime";
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
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
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

describe("Noloong app chat regression harness", () => {
  beforeEach(() => {
    window.history.replaceState(null, "", "/");
    vi.spyOn(window, "open").mockImplementation(() => null);
    tauriEvent.emitTo.mockReset();
    tauriEvent.listen.mockReset();
    tauriEvent.listeners.clear();
    tauriCore.invoke.mockReset();
    tauriCore.invoke.mockResolvedValue(undefined);
    tauriEvent.listen.mockImplementation(async (event, handler) => {
      tauriEvent.listeners.set(event, handler);
      return () => {
        tauriEvent.listeners.delete(event);
      };
    });
    resetDevInteractionRuntimeForTests();
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
    Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("starts a fake interaction runtime and shows the loaded session", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    expect(await screen.findByRole("heading", { name: "Start with a question." })).toBeInTheDocument();
    expect(screen.queryByText("Default environment")).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("default · idle");
    expect(screen.queryByRole("button", { name: "Open settings" })).not.toBeInTheDocument();
  });

  it("turns a missing interaction endpoint into an environment setup prompt", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(
      <App
        dependencies={{
          ...dependenciesFor(runtime),
          bootstrap: async () => ({
            appVersion: "test",
            interactionEndpoint: null,
            interactionStatus: { status: "unavailable" },
            locale: "en",
            profileConfigPath: null,
          }),
        }}
      />,
    );

    const heading = await screen.findByRole("heading", { name: "Choose an environment" });
    const status = heading.closest('[role="status"]');
    expect(status).toBeInstanceOf(HTMLElement);
    if (!(status instanceof HTMLElement)) {
      throw new Error("Expected the environment setup prompt to be announced as status");
    }
    expect(heading).toBeVisible();
    expect(status).toHaveTextContent("Set up a profile before starting a conversation.");
    expect(within(status).getByRole("button", { name: "Set up environment" })).toBeVisible();
    expect(document.body).not.toHaveTextContent(/runtime|endpoint/i);
  });

  it("boots the browser development preview into a ready chat runtime", async () => {
    const user = userEvent.setup();

    render(<App />);

    await composerReadyForInput();
    expect(screen.queryByRole("heading", { name: "Choose an environment" })).not.toBeInTheDocument();

    await user.type(screen.getByPlaceholderText("Write a message..."), "verify dev preview");
    await user.click(screen.getByRole("button", { name: "Send message" }));

    expect(await screen.findByText("verify dev preview")).toBeInTheDocument();
    await waitFor(
      () => expect(document.body).toHaveTextContent("dev interaction runtime"),
      { timeout: 2000 },
    );
  });

  it("keeps the browser preview stop action from completing an aborted run", async () => {
    const user = userEvent.setup();
    const finalEvents: string[] = [];
    const stopObserving = observeDevInteractionRuntimeForTests({
      onDisplayEvent(_sessionId, event) {
        if (event.type === "assistant_message_final") {
          finalEvents.push(event.message.id);
        }
      },
    });

    try {
      render(<App />);

      await user.type(await screen.findByPlaceholderText("Write a message..."), "abort preview");
      await user.click(screen.getByRole("button", { name: "Send message" }));
      await user.click(await screen.findByRole("button", { name: "Stop Run" }));

      await waitFor(() =>
        expect(screen.queryByRole("button", { name: "Stop Run" })).not.toBeInTheDocument(),
      );

      expect(finalEvents).toHaveLength(0);
    } finally {
      stopObserving();
    }
  });

  it("presents active session state as human context instead of raw status tokens", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();

    act(() => {
      runtime.emitDisplayEvent({
        type: "run_started",
        runId: "run-1",
      });
    });

    expect(await screen.findByRole("button", { name: "Stop Run" })).toBeVisible();
    expect(screen.queryByText("Default is thinking")).not.toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("default · running");
  });

  it("opens settings with the macOS settings shortcut without replacing the chat surface", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();
    await user.keyboard("{Meta>},{/Meta}");

    expect(window.open).toHaveBeenCalledWith(
      "/?surface=settings",
      "noloong-settings",
      "width=920,height=720",
    );
    expect(screen.getByRole("textbox", { name: "Write a message..." })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Open settings" })).not.toBeInTheDocument();
  });

  it("also accepts the control-comma settings shortcut fallback", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();
    await user.keyboard("{Control>},{/Control}");

    expect(window.open).toHaveBeenCalledWith(
      "/?surface=settings",
      "noloong-settings",
      "width=920,height=720",
    );
  });

  it("supports conversation keyboard commands outside the bottom composer buttons", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByRole("textbox", { name: "Write a message..." });
    screen.getByRole("button", { name: "Sessions" }).focus();

    await user.keyboard("{Meta>}l{/Meta}");
    expect(composer).toHaveFocus();

    await user.type(composer, "send from menu path");
    screen.getByRole("button", { name: "Sessions" }).focus();
    await user.keyboard("{Meta>}{Enter}{/Meta}");

    await waitFor(() => expect(runtime.promptRequests).toHaveLength(1));
    expect(runtime.promptRequests[0]).toMatchObject({
      input: { type: "text", text: "send from menu path" },
    });

    act(() => {
      runtime.emitDisplayEvent({
        type: "run_started",
        runId: "run-1",
      });
    });

    await user.keyboard("{Escape}");

    await waitFor(() => expect(runtime.abortRequests).toHaveLength(1));
    expect(runtime.abortRequests[0]).toMatchObject({ sessionId: "session-1" });
  });

  it("bridges native conversation menu events into the chat surface", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByRole("textbox", { name: "Write a message..." });
    const listener = tauriEvent.listeners.get("noloong-conversation-menu-command");
    if (!listener) {
      throw new Error("Expected the chat surface to listen for conversation menu commands");
    }
    vi.spyOn(document, "hasFocus").mockReturnValue(true);

    act(() => {
      listener({ payload: "focus-composer" });
    });
    expect(composer).toHaveFocus();

    await user.type(composer, "send from native menu");
    vi.mocked(document.hasFocus).mockReturnValue(false);
    act(() => {
      listener({ payload: "send-message" });
    });

    expect(runtime.promptRequests).toHaveLength(0);
    expect(composer).toHaveValue("send from native menu");

    vi.mocked(document.hasFocus).mockReturnValue(true);
    act(() => {
      listener({ payload: "send-message" });
    });

    await waitFor(() => expect(runtime.promptRequests).toHaveLength(1));
    expect(runtime.promptRequests[0]).toMatchObject({
      input: { type: "text", text: "send from native menu" },
    });

    await user.type(composer, "draft to clear");
    act(() => {
      listener({ payload: "clear-composer" });
    });

    expect(composer).toHaveValue("");
  });

  it("renders settings as a dedicated surface without a return-to-chat control", async () => {
    window.history.replaceState(null, "", "/?surface=settings");
    const runtime = new FakeInteractionRuntime(emptySession());

    render(<App dependencies={dependenciesFor(runtime)} />);

    expect(await screen.findByRole("heading", { name: "Provider" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Start with a question." })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back to chat" })).not.toBeInTheDocument();
  });

  it("reconnects the main chat runtime after a settings-window restart event", async () => {
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    const runtime = new FakeInteractionRuntime(emptySession());
    const createInteractionClient = vi.fn(runtime.createClient);
    const connectDisplayStream = vi.fn(runtime.connectDisplayStream);

    render(
      <App
        dependencies={{
          ...dependenciesFor(runtime),
          createInteractionClient,
          connectInteractionDisplayStream: connectDisplayStream,
        }}
      />,
    );

    await composerReadyForInput();
    expect(createInteractionClient).toHaveBeenCalledWith({
      wsUrl: "ws://127.0.0.1:7777/jsonrpc/ws",
    });

    const restartListener = tauriEvent.listeners.get("noloong-runtime-restarted");
    if (!restartListener) {
      throw new Error("Expected the main window to listen for runtime restart events");
    }

    act(() => {
      restartListener({
        payload: {
          interactionEndpoint: { wsUrl: "ws://127.0.0.1:8888/jsonrpc/ws" },
          interactionStatus: {
            status: "ready",
            serverName: "restarted-runtime",
            protocolVersion: "test-2",
            profiles: [{ profileId: "default", displayName: "Default" }],
          },
        },
      });
    });

    await waitFor(() =>
      expect(createInteractionClient).toHaveBeenCalledWith({
        wsUrl: "ws://127.0.0.1:8888/jsonrpc/ws",
      }),
    );
  });

  it("keeps session controls inside the composer capsule with accessible names", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await composerReadyForInput();
    const sessionControls = screen.getByRole("group", { name: "Session controls" });

    expect(within(sessionControls).getByRole("button", { name: "Sessions" })).toBeInTheDocument();
    expect(within(sessionControls).getByRole("button", { name: "Create session" })).toBeInTheDocument();
    expect(within(sessionControls).queryByRole("button", { name: "Open settings" })).not.toBeInTheDocument();

    await user.click(within(sessionControls).getByRole("button", { name: "Sessions" }));

    const dialog = screen.getByRole("dialog", { name: "Sessions" });
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Close sessions" })).toHaveFocus();
    expect(within(dialog).getByText("Default environment")).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent("default · idle");
    expect(screen.queryByRole("group", { name: "Session controls" })).not.toBeInTheDocument();
  });

  it("opens the sessions panel as a modal and restores focus when it closes", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const sessionsButton = await screen.findByRole("button", { name: "Sessions" });
    await user.click(sessionsButton);

    const dialog = screen.getByRole("dialog", { name: "Sessions" });
    expect(dialog).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Close sessions" })).toHaveFocus();

    await user.keyboard("{Escape}");

    expect(screen.queryByRole("dialog", { name: "Sessions" })).not.toBeInTheDocument();
    expect(sessionsButton).toHaveFocus();
  });

  it("closes the sessions panel from the standard close control", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const sessionsButton = await screen.findByRole("button", { name: "Sessions" });
    await user.click(sessionsButton);

    const dialog = screen.getByRole("dialog", { name: "Sessions" });
    await user.click(within(dialog).getByRole("button", { name: "Close sessions" }));

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
    expect(screen.queryByRole("group", { name: "Session controls" })).not.toBeInTheDocument();
  });

  it("shows the local user message immediately after sending", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByPlaceholderText("Write a message...");
    await user.type(composer, "hello from user");
    await user.click(screen.getByRole("button", { name: "Send message" }));

    expect(screen.getByText("hello from user")).toBeInTheDocument();
    expect(runtime.promptRequests[0]).toMatchObject({
      sessionId: "session-1",
      input: { type: "text", text: "hello from user" },
    });
  });

  it("reveals the composer expander when compact input exceeds the short-form threshold", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByPlaceholderText("Write a message...");
    expect(screen.queryByRole("button", { name: "Expand composer" })).not.toBeInTheDocument();
    await user.type(
      composer,
      "This is a deliberately long composer input that should move beyond the compact short-form threshold.",
    );

    await waitFor(() =>
      expect(screen.getByRole("button", { name: "Expand composer" })).toBeInTheDocument(),
    );

    await user.click(screen.getByRole("button", { name: "Expand composer" }));
    expect(screen.getByRole("button", { name: "Collapse composer" })).toBeInTheDocument();
  });

  it("keeps multiline composer input editable after expansion", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    const composer = await screen.findByRole("textbox", { name: "Write a message..." });
    await user.type(composer, "first line{Shift>}{Enter}{/Shift}second line");

    expect(await screen.findByRole("button", { name: "Expand composer" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Expand composer" }));
    expect(screen.getByRole("button", { name: "Collapse composer" })).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "Write a message..." })).toHaveValue(
      "first line\nsecond line",
    );

    await user.type(screen.getByRole("textbox", { name: "Write a message..." }), " continues");
    await user.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => expect(runtime.promptRequests).toHaveLength(1));
    expect(runtime.promptRequests[0]).toMatchObject({
      input: { type: "text", text: "first line\nsecond line continues" },
    });
    const sentMessage = screen.getByText("first line second line continues");
    expect(sentMessage.textContent).toBe("first line\nsecond line continues");
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
    await user.click(screen.getByRole("button", { name: "Send message" }));

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
    await user.click(screen.getByRole("button", { name: "Send message" }));

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
    await user.click(screen.getByRole("button", { name: "Send message" }));

    act(() => runtime.emitAssistantDelta("first"));
    await expectVisibleText("first");
    expect(document.body).not.toHaveTextContent("first second");

    act(() => runtime.emitAssistantDelta(" second"));
    await expectVisibleText("first second");
  });

  it("keeps tool audit identifiers hidden until the activity row is expanded", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

    render(<App dependencies={dependenciesFor(runtime)} />);

    await screen.findByPlaceholderText("Write a message...");
    await composerReadyForInput();

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

    expect(screen.getByText("Inspecting preview")).toBeVisible();
    expect(screen.getByText("Done")).toBeVisible();
    expect(screen.getByText("Viewport check complete.")).toBeVisible();
    expect(screen.getByText("desktop.preview.inspect")).not.toBeVisible();
    expect(screen.getByText("Captured viewport metrics.")).not.toBeVisible();

    await user.click(screen.getByText("Inspecting preview"));

    expect(screen.getByText("desktop.preview.inspect")).toBeVisible();
    expect(screen.getByText("Captured viewport metrics.")).toBeVisible();
  });

  it("renders successful tools during active reasoning as subordinate activity", async () => {
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
        text: "Reading the current UI state.",
      });
      runtime.emitDisplayEvent({
        type: "tool_started",
        toolCallId: "tool-1",
        toolName: "desktop.preview.inspect",
      });
      runtime.emitDisplayEvent({
        type: "tool_started",
        toolCallId: "tool-2",
        toolName: "host.exec.start",
      });
      runtime.emitDisplayEvent({
        type: "tool_completed",
        toolCallId: "tool-3",
        output: { content: [{ type: "text", text: "Permission denied." }], isError: true },
      });
    });

    const toolRows = document.querySelectorAll(".tool-activity");
    expect(toolRows).toHaveLength(3);
    expect(toolRows[0]).toHaveClass("tool-activity-subordinate");
    expect(toolRows[1]).toHaveClass("tool-activity-subordinate");
    expect(toolRows[2]).not.toHaveClass("tool-activity-subordinate");
    expect(toolRows[2]).toHaveTextContent("Failed");
    expect(toolRows[2]).not.toHaveTextContent("Done");
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
    await user.click(screen.getByRole("button", { name: "Send message" }));

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
    await user.click(screen.getByRole("button", { name: "Send message" }));

    act(() => runtime.emitAssistantDelta("after-send-follow"));

    await waitFor(() => expect(transcript.scrollTop).toBe(1000));
  });

  it("collapses live reasoning into an expandable completed status", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

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
    expect(screen.getByText("Thought for 2 seconds")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Show details" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Reasoning Summary" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Show details" }));

    expect(screen.getByRole("button", { name: "Hide details" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Reasoning Summary" })).toBeInTheDocument();
  });

  it("keeps completed reasoning expandable when raw text exists", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

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
        text: "summary only",
      });
      runtime.emitDisplayEvent({
        type: "thought_delta",
        runId: "run-1",
        thoughtId: "thought-1",
        kind: "raw",
        text: "raw detail",
      });
      runtime.emitDisplayEvent({
        type: "thought_completed",
        runId: "run-1",
        thoughtId: "thought-1",
        elapsedMs: 2100,
      });
    });

    expect(screen.getByText("Thought for 2 seconds")).toBeInTheDocument();
    expect(screen.queryByText("summary only")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Show details" }));

    expect(screen.getByText("summary only")).toBeInTheDocument();
    expect(screen.getByText("raw detail")).toBeInTheDocument();
  });

  it("does not duplicate raw-only completed reasoning when expanded", async () => {
    const runtime = new FakeInteractionRuntime(emptySession());
    const user = userEvent.setup();

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
        kind: "raw",
        text: "raw detail",
      });
      runtime.emitDisplayEvent({
        type: "thought_completed",
        runId: "run-1",
        thoughtId: "thought-1",
        elapsedMs: 2100,
      });
    });

    await user.click(screen.getByRole("button", { name: "Show details" }));

    expect(screen.getAllByText("raw detail")).toHaveLength(1);
  });

});

function dependenciesFor(runtime: FakeInteractionRuntime, locale: "en" | "zh" = "en") {
  return {
    bootstrap: async () => runtime.bootstrap(locale),
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

async function composerReadyForInput(placeholder = "Write a message..."): Promise<void> {
  await screen.findByPlaceholderText(placeholder);
}

async function expectVisibleText(text: string): Promise<void> {
  await waitFor(() => expect(document.body).toHaveTextContent(text));
}
