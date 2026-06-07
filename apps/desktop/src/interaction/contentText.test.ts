import { describe, expect, it } from "vitest";
import type { AppMessage } from "../generated/contracts";
import { readableJson, textFromMessage } from "./contentText";

describe("content text helpers", () => {
  it("extracts text blocks from agent messages", () => {
    expect(
      textFromMessage({
        id: "message-1",
        role: "assistant",
        content: [
          { type: "text", text: "hello" },
          { type: "thinking", thinking: { kind: "summary", text: "internal" } },
          { type: "text", text: "world" },
        ],
      } satisfies AppMessage),
    ).toBe("hello\nworld");
  });

  it("serializes structured values as compact readable JSON", () => {
    expect(readableJson({ ok: true })).toBe('{"ok":true}');
    expect(readableJson("already text")).toBe("already text");
    expect(readableJson(undefined)).toBe("");
  });
});
