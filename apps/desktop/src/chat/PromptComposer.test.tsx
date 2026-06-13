// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createI18n } from "../i18n";
import { dispatchConversationCommand } from "./conversationCommands";
import { PromptComposer } from "./PromptComposer";

vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: () => Promise.resolve(() => undefined),
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

describe("PromptComposer", () => {
  afterEach(() => {
    cleanup();
    vi.clearAllMocks();
  });

  it("uses the send control as the stop control while a run is active", async () => {
    const user = userEvent.setup();
    const onAbortRun = vi.fn().mockResolvedValue(undefined);
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <PromptComposer
        disabled
        i18n={createI18n("en")}
        onAbortRun={onAbortRun}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={onSubmit}
        placeholder="Write a message..."
      />,
    );

    const stop = screen.getByRole("button", { name: "Stop" });
    expect(screen.queryByRole("button", { name: "Send message" })).not.toBeInTheDocument();

    await user.click(stop);

    expect(onAbortRun).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("reports command availability without knowing about the native menu", async () => {
    const user = userEvent.setup();
    const onCommandAvailabilityChange = vi.fn();
    const onAbortRun = vi.fn().mockResolvedValue(undefined);
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    const { rerender } = render(
      <PromptComposer
        disabled={false}
        i18n={createI18n("en")}
        onCommandAvailabilityChange={onCommandAvailabilityChange}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={onSubmit}
        placeholder="Write a message..."
      />,
    );

    await waitFor(() =>
      expect(onCommandAvailabilityChange).toHaveBeenCalledWith({
        canFocusComposer: true,
        canSendMessage: false,
        canStopResponse: false,
      }),
    );

    await user.type(screen.getByRole("textbox", { name: "Write a message..." }), "hello");
    await waitFor(() =>
      expect(onCommandAvailabilityChange).toHaveBeenCalledWith({
        canFocusComposer: true,
        canSendMessage: true,
        canStopResponse: false,
      }),
    );

    rerender(
      <PromptComposer
        disabled
        i18n={createI18n("en")}
        onAbortRun={onAbortRun}
        onCommandAvailabilityChange={onCommandAvailabilityChange}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={onSubmit}
        placeholder="Write a message..."
      />,
    );

    await waitFor(() =>
      expect(onCommandAvailabilityChange).toHaveBeenCalledWith({
        canFocusComposer: false,
        canSendMessage: false,
        canStopResponse: true,
      }),
    );
  });

  it("handles conversation commands through one command event path", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    const { rerender } = render(
      <PromptComposer
        disabled={false}
        i18n={createI18n("en")}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={onSubmit}
        placeholder="Write a message..."
      />,
    );

    const textarea = screen.getByRole("textbox", { name: "Write a message..." });
    screen.getByRole("button", { name: "Sessions" }).focus();
    act(() => dispatchConversationCommand("focus-composer"));
    expect(textarea).toHaveFocus();

    await user.type(textarea, "send through command");
    act(() => dispatchConversationCommand("send-message"));
    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({
        text: "send through command",
        attachments: [],
      }),
    );

    const onAbortRun = vi.fn().mockResolvedValue(undefined);
    rerender(
      <PromptComposer
        disabled
        i18n={createI18n("en")}
        onAbortRun={onAbortRun}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={onSubmit}
        placeholder="Write a message..."
      />,
    );

    act(() => dispatchConversationCommand("stop-response"));
    await waitFor(() => expect(onAbortRun).toHaveBeenCalledTimes(1));
  });

  it("routes the composer return shortcut through the command dispatcher", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);

    render(
      <PromptComposer
        disabled={false}
        i18n={createI18n("en")}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={onSubmit}
        placeholder="Write a message..."
      />,
    );

    const textarea = screen.getByRole("textbox", { name: "Write a message..." });
    await user.type(textarea, "keyboard command");
    await user.keyboard("{Meta>}{Enter}{/Meta}");

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith({
        text: "keyboard command",
        attachments: [],
      }),
    );
  });

  it("shows scroll edge affordances only where expanded input has clipped content", async () => {
    const user = userEvent.setup();

    render(
      <PromptComposer
        disabled={false}
        i18n={createI18n("en")}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={vi.fn()}
        placeholder="Write a message..."
      />,
    );

    const textarea = screen.getByRole("textbox", { name: "Write a message..." });
    await user.type(textarea, "first line{Shift>}{Enter}{/Shift}second line");
    await user.click(await screen.findByRole("button", { name: "Expand composer" }));

    const editorShell = textarea.closest(".composer-editor-shell");
    expect(editorShell).toBeTruthy();

    setScrollMetrics(textarea, { clientHeight: 120, scrollHeight: 280 });

    textarea.scrollTop = 0;
    fireEvent.scroll(textarea);
    await waitFor(() => expect(editorShell).toHaveClass("fade-bottom"));
    expect(editorShell).not.toHaveClass("fade-top");

    textarea.scrollTop = 80;
    fireEvent.scroll(textarea);
    await waitFor(() => expect(editorShell).toHaveClass("fade-top"));
    expect(editorShell).toHaveClass("fade-bottom");

    textarea.scrollTop = 160;
    fireEvent.scroll(textarea);
    await waitFor(() => expect(editorShell).not.toHaveClass("fade-bottom"));
    expect(editorShell).toHaveClass("fade-top");

    setScrollMetrics(textarea, { clientHeight: 120, scrollHeight: 120 });
    textarea.scrollTop = 0;
    fireEvent.scroll(textarea);
    await waitFor(() => expect(editorShell).not.toHaveClass("fade-top"));
    expect(editorShell).not.toHaveClass("fade-bottom");
  });
});

function setScrollMetrics(
  element: HTMLElement,
  metrics: { clientHeight: number; scrollHeight: number },
): void {
  Object.defineProperty(element, "clientHeight", { configurable: true, value: metrics.clientHeight });
  Object.defineProperty(element, "scrollHeight", { configurable: true, value: metrics.scrollHeight });
}
