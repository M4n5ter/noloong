// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createI18n } from "../i18n";
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
    vi.clearAllMocks();
  });

  it("shows scroll edge affordances only where expanded input has clipped content", async () => {
    const user = userEvent.setup();

    render(
      <PromptComposer
        disabled={false}
        i18n={createI18n("en")}
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
