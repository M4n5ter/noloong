import { useEffect, useRef, useState } from "react";
import { isFullHtmlDocument, isHtmlHeadSealed } from "./streaming";
import { PlainCodeBlock } from "./PlainCodeBlock";

const defaultSandbox = "allow-scripts";
const maxSrcDocLength = 5 * 1024 * 1024;

export function HtmlPreview({
  content,
  streaming,
}: {
  content: string;
  streaming: boolean;
}) {
  const trimmed = content.trim();
  const visibleContent = useThrottledHtmlContent(trimmed, streaming);
  const previewable = isFullHtmlDocument(visibleContent);

  if (!previewable || visibleContent.length > maxSrcDocLength) {
    return <PlainCodeBlock code={content} language="html" />;
  }

  if (streaming && !isHtmlHeadSealed(visibleContent)) {
    return <PlainCodeBlock code={content} language="html" />;
  }

  return (
    <iframe
      className="markdown-html-preview"
      sandbox={defaultSandbox}
      srcDoc={visibleContent}
      title="HTML preview"
    />
  );
}

function useThrottledHtmlContent(content: string, streaming: boolean): string {
  const [visibleContent, setVisibleContent] = useState(content);
  const latestContentRef = useRef(content);
  const lastCommitRef = useRef(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    latestContentRef.current = content;
  }, [content]);

  useEffect(
    () => () => {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
    },
    [],
  );

  useEffect(() => {
    if (!streaming) {
      if (timerRef.current) {
        clearTimeout(timerRef.current);
        timerRef.current = null;
      }
      lastCommitRef.current = Date.now();
      setVisibleContent(content);
      return;
    }

    const throttleMs = 250;
    const currentTime = Date.now();
    const elapsed = currentTime - lastCommitRef.current;
    if (elapsed >= throttleMs) {
      lastCommitRef.current = currentTime;
      setVisibleContent(content);
      return;
    }

    if (timerRef.current === null) {
      timerRef.current = setTimeout(() => {
        lastCommitRef.current = Date.now();
        timerRef.current = null;
        setVisibleContent(latestContentRef.current);
      }, throttleMs - elapsed);
    }
  }, [content, streaming]);

  return visibleContent;
}
