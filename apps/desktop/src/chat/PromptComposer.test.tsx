// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { open } from "@tauri-apps/plugin-dialog";
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

    const stop = screen.getByRole("button", { name: "Stop Run" });
    expect(screen.queryByRole("button", { name: "Send message" })).not.toBeInTheDocument();

    await user.click(stop);

    expect(onAbortRun).toHaveBeenCalledTimes(1);
    expect(onSubmit).not.toHaveBeenCalled();
  });

  it("gives icon-only composer controls native hover help", async () => {
    const user = userEvent.setup();
    const { rerender } = render(
      <PromptComposer
        disabled={false}
        i18n={createI18n("en")}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={vi.fn()}
        placeholder="Write a message..."
      />,
    );

    expect(screen.getByRole("button", { name: "Attach files" })).toHaveAttribute(
      "title",
      "Attach files",
    );
    expect(screen.getByRole("button", { name: "Send message" })).toHaveAttribute(
      "title",
      "Send message",
    );
    expect(screen.getByRole("button", { name: "Sessions" })).toHaveAttribute("title", "Sessions");
    expect(screen.getByRole("button", { name: "Create session" })).toHaveAttribute(
      "title",
      "Create session",
    );
    const textarea = screen.getByRole("textbox", { name: "Write a message..." });
    await user.type(
      textarea,
      "This is a deliberately long composer input that should reveal the collapse control in the expanded editor.",
    );
    expect(screen.getByRole("button", { name: "Collapse composer" })).toHaveAttribute(
      "title",
      "Collapse composer",
    );
    await user.click(screen.getByRole("button", { name: "Collapse composer" }));
    expect(screen.getByRole("button", { name: "Expand composer" })).toHaveAttribute(
      "title",
      "Expand composer",
    );

    rerender(
      <PromptComposer
        disabled
        i18n={createI18n("en")}
        onAbortRun={vi.fn().mockResolvedValue(undefined)}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={vi.fn()}
        placeholder="Write a message..."
      />,
    );

    expect(screen.getByRole("button", { name: "Stop Run" })).toHaveAttribute("title", "Stop Run");
  });

  it("gives attachment removal native hover help", async () => {
    const user = userEvent.setup();
    vi.mocked(open).mockResolvedValue(["/tmp/design reference.png"]);

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

    await user.click(screen.getByRole("button", { name: "Attach files" }));

    expect(await screen.findByText("design reference.png")).toBeVisible();
    const remove = screen.getByRole("button", { name: "Remove design reference.png" });
    expect(remove).toHaveAttribute("title", "Remove design reference.png");

    await user.click(remove);

    await waitFor(() => expect(screen.queryByText("design reference.png")).not.toBeInTheDocument());
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
        canClearComposer: false,
      }),
    );

    await user.type(screen.getByRole("textbox", { name: "Write a message..." }), "hello");
    await waitFor(() =>
      expect(onCommandAvailabilityChange).toHaveBeenCalledWith({
        canFocusComposer: true,
        canSendMessage: true,
        canStopResponse: false,
        canClearComposer: true,
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
        canClearComposer: false,
      }),
    );
  });

  it("shows input focus treatment for keyboard and command focus only", async () => {
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
    const capsule = textarea.closest(".composer-capsule");
    const editorShell = textarea.closest(".composer-editor-shell");
    expect(capsule).toBeTruthy();
    expect(editorShell).toBeTruthy();

    await user.click(textarea);
    expect(editorShell).not.toHaveClass("keyboard-focus");

    act(() => dispatchConversationCommand("focus-composer"));
    expect(textarea).toHaveFocus();
    expect(editorShell).toHaveClass("keyboard-focus");

    await user.click(textarea);
    expect(editorShell).not.toHaveClass("keyboard-focus");

    await user.tab({ shift: true });
    await user.click(capsule as HTMLElement);
    expect(textarea).toHaveFocus();
    expect(editorShell).not.toHaveClass("keyboard-focus");

    await user.click(textarea);
    await user.click(screen.getByRole("button", { name: "Sessions" }));
    await user.tab();
    await user.tab();
    expect(textarea).toHaveFocus();
    expect(editorShell).toHaveClass("keyboard-focus");

    await user.click(screen.getByRole("button", { name: "Sessions" }));
    act(() => dispatchConversationCommand("focus-composer"));
    expect(textarea).toHaveFocus();
    expect(editorShell).toHaveClass("keyboard-focus");

    await user.tab({ shift: true });
    await user.tab();
    expect(textarea).toHaveFocus();
    expect(editorShell).toHaveClass("keyboard-focus");
  });

  it("does not show input focus treatment when command focus cannot reach a disabled input", () => {
    render(
      <PromptComposer
        disabled
        i18n={createI18n("en")}
        onCreateSession={vi.fn()}
        onOpenSessions={vi.fn()}
        onSubmit={vi.fn()}
        placeholder="Write a message..."
      />,
    );

    const textarea = screen.getByRole("textbox", { name: "Write a message..." });
    const editorShell = textarea.closest(".composer-editor-shell");
    expect(editorShell).toBeTruthy();

    act(() => dispatchConversationCommand("focus-composer"));

    expect(textarea).not.toHaveFocus();
    expect(editorShell).not.toHaveClass("keyboard-focus");
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

  it("clears draft text and attachments through the conversation command path", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn().mockResolvedValue(undefined);
    vi.mocked(open).mockResolvedValue(["/tmp/context.txt"]);

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
    await user.type(textarea, "draft text");
    await user.click(screen.getByRole("button", { name: "Attach files" }));

    expect(await screen.findByText("context.txt")).toBeVisible();

    act(() => dispatchConversationCommand("clear-composer"));

    expect(textarea).toHaveValue("");
    expect(textarea).toHaveFocus();
    expect(screen.queryByText("context.txt")).not.toBeInTheDocument();
    expect(onSubmit).not.toHaveBeenCalled();
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
    expect(textarea.closest(".composer-editor-shell")).toHaveClass("expanded");

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

  it("promotes multiline input into the expanded editor", async () => {
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

    expect(screen.getByRole("button", { name: "Collapse composer" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );

    await user.click(screen.getByRole("button", { name: "Collapse composer" }));

    expect(screen.queryByRole("textbox", { name: "Write a message..." })).not.toBeInTheDocument();
    const draftPreview = screen.getByRole("button", { name: "first line second line" });
    expect(draftPreview).toHaveAttribute("aria-controls");

    await user.click(draftPreview);

    expect(screen.getByRole("textbox", { name: "Write a message..." })).toHaveValue(
      "first line\nsecond line",
    );
  });

  it("promotes long input into the expanded editor", async () => {
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
    await user.type(
      textarea,
      "This is a deliberately long composer input that should reveal the expansion control before it turns into a larger editing surface.",
    );
    expect(screen.getByRole("button", { name: "Collapse composer" })).toBeInTheDocument();
    expect(textarea).toHaveValue(
      "This is a deliberately long composer input that should reveal the expansion control before it turns into a larger editing surface.",
    );
  });
});

function setScrollMetrics(
  element: HTMLElement,
  metrics: { clientHeight: number; scrollHeight: number },
): void {
  Object.defineProperty(element, "clientHeight", { configurable: true, value: metrics.clientHeight });
  Object.defineProperty(element, "scrollHeight", { configurable: true, value: metrics.scrollHeight });
}
