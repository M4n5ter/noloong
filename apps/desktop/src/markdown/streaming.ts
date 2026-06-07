import type { Element, ElementContent, Root } from "hast";
import type { BuildVisitor } from "unist-util-visit";
import { visit } from "unist-util-visit";

export type StreamSmoothingPreset = "realtime" | "balanced" | "silky";

export type BlockInfo = {
  content: string;
  startOffset: number;
};

export type BlockState = "revealed" | "animating" | "streaming" | "queued";

export type StreamAnimatedOptions = {
  births?: number[];
  fadeDuration?: number;
  nowMs?: number;
  revealed?: boolean;
};

const blockTags = new Set(["p", "h1", "h2", "h3", "h4", "h5", "h6", "li"]);
const skipTags = new Set(["pre", "code", "table", "svg"]);

export function countChars(text: string): number {
  return [...text].length;
}

export function findOpenFenceLanguage(content: string): string | null {
  let inFence = false;
  let language = "";
  let index = 0;

  while (index < content.length) {
    const nextLine = content.indexOf("\n", index);
    const lineEnd = nextLine === -1 ? content.length : nextLine;
    const line = content.slice(index, lineEnd);

    if (line.startsWith("```")) {
      if (inFence) {
        inFence = false;
        language = "";
      } else {
        inFence = true;
        language = line.slice(3).trim().toLowerCase();
      }
    }

    if (nextLine === -1) {
      break;
    }
    index = nextLine + 1;
  }

  return inFence ? language : null;
}

export function resolveBlockAnimationMeta({
  currentCharDelay,
  fadeDuration,
  lastElapsedMs,
  previousCharDelay,
  state,
}: {
  currentCharDelay: number;
  fadeDuration: number;
  lastElapsedMs: number;
  previousCharDelay?: number;
  state: BlockState;
}): { charDelay: number; settled: boolean } {
  const active = state === "animating" || state === "streaming";
  return {
    charDelay: active ? currentCharDelay : (previousCharDelay ?? currentCharDelay),
    settled: state === "revealed" && lastElapsedMs >= fadeDuration,
  };
}

export function rehypeStreamAnimated(options: StreamAnimatedOptions = {}) {
  const { births, fadeDuration = 280, nowMs, revealed = false } = options;
  const hasBirths = !revealed && Array.isArray(births) && typeof nowMs === "number";

  return (tree: Root) => {
    let globalCharIndex = 0;

    const shouldSkip = (node: Element): boolean => {
      if (skipTags.has(node.tagName)) {
        return true;
      }
      const className = node.properties?.className;
      if (Array.isArray(className)) {
        return className.some((value) => String(value).includes("katex"));
      }
      return typeof className === "string" && className.includes("katex");
    };

    const wrapText = (node: Element) => {
      const nextChildren: ElementContent[] = [];

      for (const child of node.children) {
        if (child.type === "text") {
          for (const char of child.value) {
            let className = "stream-char";
            let delay: number | undefined;

            if (revealed) {
              className = "stream-char stream-char-revealed";
            } else if (hasBirths) {
              const birthTs = births[globalCharIndex];
              if (birthTs === undefined) {
                className = "stream-char stream-char-revealed";
              } else {
                const elapsed = nowMs - birthTs;
                if (elapsed >= fadeDuration) {
                  className = "stream-char stream-char-revealed";
                } else {
                  delay = -elapsed;
                }
              }
            }

            const properties: Record<string, string> = { className };
            if (delay !== undefined && delay !== 0) {
              properties.style = `animation-delay:${delay}ms`;
            }
            nextChildren.push({
              children: [{ type: "text", value: char }],
              properties,
              tagName: "span",
              type: "element",
            });
            globalCharIndex += 1;
          }
        } else if (child.type === "element") {
          if (!shouldSkip(child)) {
            wrapText(child);
          }
          nextChildren.push(child);
        } else {
          nextChildren.push(child);
        }
      }

      node.children = nextChildren;
    };

    visit(tree, "element", ((node: Element) => {
      if (shouldSkip(node)) {
        return "skip";
      }
      if (blockTags.has(node.tagName)) {
        wrapText(node);
        return "skip";
      }
      return undefined;
    }) as BuildVisitor<Root, "element">);
  };
}

export function isFullHtmlDocument(content: string): boolean {
  const head = content.slice(0, 1024).toLowerCase();
  return /<!doctype\s+html\b|<html(?:\s|>)/i.test(head);
}

export function isHtmlHeadSealed(content: string): boolean {
  return /<\/head\s*>|<body[\s>]|<\/style\s*>/i.test(content);
}
