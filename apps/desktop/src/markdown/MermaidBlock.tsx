import { useEffect, useId, useMemo, useRef, useState } from "react";
import { PlainCodeBlock } from "./PlainCodeBlock";

const cache = new Map<string, Promise<string>>();
let mermaidModulePromise: Promise<typeof import("mermaid").default> | null = null;

function loadMermaid(): Promise<typeof import("mermaid").default> {
  mermaidModulePromise ??= import("mermaid").then((module) => module.default);
  return mermaidModulePromise;
}

export function MermaidBlock({
  content,
  streaming,
}: {
  content: string;
  streaming: boolean;
}) {
  const id = useSafeMermaidId();
  const [svg, setSvg] = useState("");
  const latestContentRef = useRef(content);
  const trimmed = content.trim();

  useEffect(() => {
    latestContentRef.current = trimmed;
  }, [trimmed]);

  useEffect(() => {
    if (trimmed.length === 0) {
      setSvg("");
      return;
    }

    const delayMs = streaming ? 300 : 0;
    let cancelled = false;
    const timer = setTimeout(() => {
      void renderMermaid(trimmed, id)
        .then((nextSvg) => {
          if (!cancelled && latestContentRef.current === trimmed) {
            setSvg(nextSvg);
          }
        })
        .catch(() => {
          if (!cancelled && !streaming) {
            setSvg("");
          }
        });
    }, delayMs);

    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [id, streaming, trimmed]);

  if (!svg) {
    return <PlainCodeBlock code={content} language="mermaid" />;
  }

  return (
    <div
      className="markdown-mermaid"
      dangerouslySetInnerHTML={{ __html: svg }}
      data-testid="markdown-mermaid"
    />
  );
}

async function renderMermaid(content: string, id: string): Promise<string> {
  const cacheKey = `${id}:${content}`;
  const cached = cache.get(cacheKey);
  if (cached) {
    return cached;
  }

  const promise = (async () => {
    const mermaid = await loadMermaid();
    mermaid.initialize({
      securityLevel: "strict",
      startOnLoad: false,
      theme: "dark",
    });
    await mermaid.parse(content);
    const { svg } = await mermaid.render(id, content);
    return svg;
  })();
  cache.set(cacheKey, promise);
  trimCache();
  return promise;
}

function trimCache(): void {
  if (cache.size <= 100) {
    return;
  }
  const firstKey = cache.keys().next().value;
  if (firstKey) {
    cache.delete(firstKey);
  }
}

function useSafeMermaidId(): string {
  const id = useId();
  return useMemo(() => `mermaid-${id.replaceAll(/[^a-zA-Z0-9_-]/g, "-")}`, [id]);
}
