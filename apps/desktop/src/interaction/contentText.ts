import type { AppContentBlock, AppMessage, JsonUnknown } from "../generated/contracts";

export function textFromMessage(message: AppMessage): string {
  return textFromContentBlocks(message.content ?? []);
}

export function textFromContentBlocks(blocks: AppContentBlock[]): string {
  return blocks
    .flatMap((block) => {
      switch (block.type) {
        case "text":
          return [block.text];
        default:
          return [];
      }
    })
    .join("\n")
    .trim();
}

export function readableJson(value: JsonUnknown | undefined): string {
  if (value == null) {
    return "";
  }
  if (typeof value === "string") {
    return value;
  }
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}
