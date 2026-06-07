// @vitest-environment jsdom

import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { MarkdownMessage } from "./MarkdownMessage";
import { MarkdownRenderer } from "./MarkdownRenderer";

describe("MarkdownRenderer", () => {
  beforeEach(() => {
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      callback(16);
      return 1;
    });
    vi.stubGlobal("cancelAnimationFrame", () => undefined);
  });

  afterEach(() => {
    cleanup();
    vi.unstubAllGlobals();
  });

  it("renders rich markdown without requiring lobe-ui", () => {
    render(
      <MarkdownRenderer>
        {"# Title\n\n- [x] task\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n**bold**"}
      </MarkdownRenderer>,
    );

    expect(screen.getByRole("heading", { name: "Title" })).toBeInTheDocument();
    expect(screen.getByRole("table")).toHaveTextContent("A");
    expect(document.body).toHaveTextContent("bold");
  });

  it("does not inject raw html into the host DOM", () => {
    render(<MarkdownRenderer>{"<script>window.bad = true</script><div>raw</div>"}</MarkdownRenderer>);

    expect(document.querySelector("script")).toBeNull();
    expect(document.querySelector("div.raw")).toBeNull();
  });

  it("routes complete fenced html documents to a sandboxed iframe preview", async () => {
    render(
      <MarkdownRenderer>
        {
          "```html\n<!doctype html><html><head></head><body><h1>Preview</h1></body></html>\n```"
        }
      </MarkdownRenderer>,
    );

    const iframe = await screen.findByTitle("HTML preview");
    expect(iframe).toHaveAttribute("sandbox", "allow-scripts");
    expect(iframe.getAttribute("sandbox")).not.toContain("allow-same-origin");
  });

  it("renders math with KaTeX", () => {
    render(<MarkdownRenderer>{"Inline math $x^2 + y^2 = z^2$."}</MarkdownRenderer>);

    expect(document.querySelector(".katex")).toBeInTheDocument();
  });

  it("keeps streaming mermaid source readable while the diagram is incomplete", async () => {
    render(<MarkdownRenderer streaming>{"```mermaid\ngraph TD\nA -->"}</MarkdownRenderer>);

    await waitFor(() => expect(document.body).toHaveTextContent("graph TD"));
  });

  it("keeps streaming text visible even when characters are wrapped for animation", async () => {
    const { rerender } = render(<MarkdownRenderer streaming>{""}</MarkdownRenderer>);

    rerender(<MarkdownRenderer streaming>{"streaming **markdown**"}</MarkdownRenderer>);

    await waitFor(() => expect(document.body).toHaveTextContent("streaming"));
  });

  it("settles final streaming content instead of flushing the tail immediately", () => {
    const rafCallbacks: FrameRequestCallback[] = [];
    vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => {
      rafCallbacks.push(callback);
      return rafCallbacks.length;
    });

    const { rerender } = render(<MarkdownRenderer streaming>{""}</MarkdownRenderer>);
    const liveContent = "streaming ".repeat(14);
    const finalContent = `${liveContent}\n\nfinal tail marker`;

    rerender(<MarkdownRenderer streaming>{liveContent}</MarkdownRenderer>);
    rerender(<MarkdownRenderer streaming={false}>{finalContent}</MarkdownRenderer>);

    expect(document.body).not.toHaveTextContent("final tail marker");

    act(() => {
      for (let frame = 0; frame < 240; frame += 1) {
        const callback = rafCallbacks.shift();
        if (!callback) {
          break;
        }
        callback((frame + 1) * 16);
      }
    });

    expect(document.body).toHaveTextContent("final tail marker");
  });

  it("renders assistant role markdown regardless of role casing", () => {
    render(<MarkdownMessage role="ASSISTANT" streaming={false} text="# Assistant title" />);

    expect(screen.getByRole("heading", { name: "Assistant title" })).toBeInTheDocument();
  });
});
