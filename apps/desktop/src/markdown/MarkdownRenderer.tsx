import "katex/dist/katex.min.css";

import { marked } from "marked";
import { memo, useId, useMemo, useRef, type ComponentProps, type ReactNode } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import rehypeKatex from "rehype-katex";
import remend from "remend";
import remarkBreaks from "remark-breaks";
import remarkCjkFriendly from "remark-cjk-friendly";
import remarkGfm from "remark-gfm";
import remarkMath from "remark-math";
import type { Pluggable, PluggableList } from "unified";
import { CodeBlock } from "./CodeBlock";
import {
  countChars,
  rehypeStreamAnimated,
  resolveBlockAnimationMeta,
  type BlockInfo,
} from "./streaming";
import { useSmoothStreamContent } from "./useSmoothStreamContent";
import { useStreamQueue } from "./useStreamQueue";

const streamFadeDuration = 280;
const revealedStreamPlugin: Pluggable = [rehypeStreamAnimated, { revealed: true }];
const revealedBlockPlugins: PluggableList = [rehypeKatex, revealedStreamPlugin];

type MarkdownRendererProps = {
  children: string;
  streaming?: boolean;
};

export function MarkdownRenderer({ children, streaming = false }: MarkdownRendererProps) {
  const displayedContent = useSmoothStreamContent(children, { enabled: streaming, preset: "balanced" });
  const processedContent = useMemo(() => remend(displayedContent), [displayedContent]);
  const components = useMarkdownComponents(streaming);

  if (!streaming) {
    return (
      <div className="markdown-body">
        <MarkdownBlock components={components}>{processedContent}</MarkdownBlock>
      </div>
    );
  }

  return <StreamMarkdown components={components}>{processedContent}</StreamMarkdown>;
}

function StreamMarkdown({
  children,
  components,
}: {
  children: string;
  components: Components;
}) {
  const generatedId = useId();
  const blocks = useMemo(() => tokenizeMarkdownBlocks(children), [children]);
  const { getBlockState, charDelay } = useStreamQueue(blocks);
  const blockCharDelayRef = useRef<Map<number, number>>(new Map());
  const blockBirthsRef = useRef<Map<number, number[]>>(new Map());
  const renderNow = getNow();

  const birthsForRender = useMemo(() => {
    const nextBirths = new Map<number, number[]>();
    const previousBirths = blockBirthsRef.current;

    for (const [index, block] of blocks.entries()) {
      const state = getBlockState(index);
      if (state === "queued") {
        continue;
      }

      const charCount = countChars(block.content);
      const previous = previousBirths.get(block.startOffset);
      let births: number[];
      if (previous && previous.length === charCount) {
        births = previous;
      } else if (previous && previous.length > charCount) {
        births = previous.slice(0, charCount);
      } else {
        births = previous ? previous.slice() : [];
        const cap = renderNow + streamFadeDuration;
        for (let i = births.length; i < charCount; i += 1) {
          const previousBirth = i > 0 ? (births[i - 1] ?? renderNow - charDelay) : renderNow - charDelay;
          const chained = previousBirth + charDelay;
          births.push(Math.min(cap, Math.max(chained, renderNow)));
        }
      }
      nextBirths.set(block.startOffset, births);
    }

    return nextBirths;
  }, [blocks, charDelay, getBlockState, renderNow]);

  const blockPlugins = useMemo(() => {
    const nextBlockCharDelay = new Map<number, number>();
    const plugins = new Map<number, PluggableList>();

    for (const [index, block] of blocks.entries()) {
      const state = getBlockState(index);
      const births = birthsForRender.get(block.startOffset);
      const lastBirthTs = births && births.length > 0 ? (births.at(-1) ?? renderNow) : renderNow;
      const meta = resolveBlockAnimationMeta({
        currentCharDelay: charDelay,
        fadeDuration: streamFadeDuration,
        lastElapsedMs: renderNow - lastBirthTs,
        previousCharDelay: blockCharDelayRef.current.get(block.startOffset),
        state,
      });
      nextBlockCharDelay.set(block.startOffset, meta.charDelay);
      plugins.set(
        block.startOffset,
        meta.settled
          ? revealedBlockPlugins
          : [
              rehypeKatex,
              [
                rehypeStreamAnimated,
                {
                  births,
                  fadeDuration: streamFadeDuration,
                  nowMs: renderNow,
                },
              ],
            ],
      );
    }

    blockCharDelayRef.current = nextBlockCharDelay;
    blockBirthsRef.current = birthsForRender;
    return plugins;
  }, [birthsForRender, blocks, charDelay, getBlockState, renderNow]);

  return (
    <div className="markdown-body markdown-stream">
      {blocks.map((block, index) => {
        const state = getBlockState(index);
        if (state === "queued") {
          return null;
        }

        return (
          <MarkdownBlock
            components={components}
            key={`${generatedId}-${block.startOffset}`}
            rehypePlugins={blockPlugins.get(block.startOffset)}
          >
            {block.content}
          </MarkdownBlock>
        );
      })}
    </div>
  );
}

const MarkdownBlock = memo(function MarkdownBlock({
  children,
  components,
  rehypePlugins,
}: {
  children: string;
  components: Components;
  rehypePlugins?: PluggableList;
}) {
  return (
    <ReactMarkdown
      components={components}
      rehypePlugins={rehypePlugins ?? [rehypeKatex]}
      remarkPlugins={remarkPlugins}
    >
      {children}
    </ReactMarkdown>
  );
});

function useMarkdownComponents(streaming: boolean): Components {
  return useMemo(
    () => ({
      a: ({ node: _node, ...props }: ComponentProps<"a"> & { node?: unknown }) => (
        <a {...props} rel="noreferrer" target="_blank" />
      ),
      img: ({ node: _node, ...props }: ComponentProps<"img"> & { node?: unknown }) => (
        <img {...props} loading="lazy" />
      ),
      pre: ({ node: _node, children }: { node?: unknown; children?: ReactNode }) => (
        <CodeBlock streaming={streaming}>{children}</CodeBlock>
      ),
    }),
    [streaming],
  );
}

function tokenizeMarkdownBlocks(content: string): BlockInfo[] {
  let offset = 0;
  return marked.lexer(content).map((token) => {
    const raw = token.raw || "";
    const block = { content: raw, startOffset: offset };
    offset += raw.length;
    return block;
  });
}

function getNow(): number {
  return typeof performance === "undefined" ? Date.now() : performance.now();
}

const remarkPlugins: PluggableList = [
  remarkCjkFriendly,
  remarkMath,
  [remarkGfm, { singleTilde: false }],
  remarkBreaks,
];
