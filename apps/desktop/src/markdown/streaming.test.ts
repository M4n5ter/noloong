import { describe, expect, it } from "vitest";
import {
  findOpenFenceLanguage,
  isFullHtmlDocument,
  isHtmlHeadSealed,
  resolveBlockAnimationMeta,
} from "./streaming";

describe("markdown streaming helpers", () => {
  it("tracks open fenced code language", () => {
    expect(findOpenFenceLanguage("plain text")).toBe(null);
    expect(findOpenFenceLanguage("```html\n<!doctype")).toBe("html");
    expect(findOpenFenceLanguage("```HTML\n<!doctype")).toBe("html");
    expect(findOpenFenceLanguage("```html\nx\n```")).toBe(null);
    expect(findOpenFenceLanguage("inline ``` is not a fence")).toBe(null);
  });

  it("detects previewable full html documents without accepting fragments", () => {
    expect(isFullHtmlDocument("<!doctype html><html><body>x</body></html>")).toBe(true);
    expect(isFullHtmlDocument("<html><body>x</body></html>")).toBe(true);
    expect(isFullHtmlDocument("<div>x</div>")).toBe(false);
    expect(isFullHtmlDocument("<htmlish><body>x</body></htmlish>")).toBe(false);
  });

  it("detects html streaming stability markers", () => {
    expect(isHtmlHeadSealed("<!doctype html><html><head><style>body{}")).toBe(false);
    expect(isHtmlHeadSealed("<!doctype html><html><head></head><body>")).toBe(true);
    expect(isHtmlHeadSealed("<!doctype html><style>body{}</style>")).toBe(true);
  });

  it("keeps active block animation delay and settles revealed blocks", () => {
    expect(
      resolveBlockAnimationMeta({
        currentCharDelay: 18,
        fadeDuration: 280,
        lastElapsedMs: 80,
        previousCharDelay: 12,
        state: "streaming",
      }),
    ).toEqual({ charDelay: 18, settled: false });

    expect(
      resolveBlockAnimationMeta({
        currentCharDelay: 10,
        fadeDuration: 280,
        lastElapsedMs: 320,
        previousCharDelay: 20,
        state: "revealed",
      }),
    ).toEqual({ charDelay: 20, settled: true });
  });
});
