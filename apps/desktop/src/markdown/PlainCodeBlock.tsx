export function PlainCodeBlock({ code, language }: { code: string; language: string }) {
  return (
    <pre className="markdown-code-block markdown-code-fallback" data-language={language}>
      <code>{code}</code>
    </pre>
  );
}
