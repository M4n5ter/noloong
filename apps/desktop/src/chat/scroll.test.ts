import { describe, expect, it } from "vitest";
import { isNearTranscriptBottom, scrollTranscriptToEnd } from "./scroll";

describe("chat scroll helpers", () => {
  it("detects whether transcript is close enough to the bottom", () => {
    expect(isNearTranscriptBottom({ scrollHeight: 1000, scrollTop: 620, clientHeight: 300 })).toBe(
      true,
    );
    expect(isNearTranscriptBottom({ scrollHeight: 1000, scrollTop: 500, clientHeight: 300 })).toBe(
      false,
    );
  });

  it("scrolls the transcript container itself to the end", () => {
    const transcript = { scrollHeight: 1200, scrollTop: 0 } as HTMLDivElement;

    scrollTranscriptToEnd(transcript);

    expect(transcript.scrollTop).toBe(1200);
  });
});
