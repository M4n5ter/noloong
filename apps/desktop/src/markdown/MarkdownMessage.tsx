import { MarkdownRenderer } from "./MarkdownRenderer";

export function MarkdownMessage({
  role,
  streaming,
  text,
}: {
  role: string;
  streaming: boolean;
  text: string;
}) {
  if (role.toLowerCase() !== "assistant") {
    return <p>{text}</p>;
  }

  return <MarkdownRenderer streaming={streaming}>{text}</MarkdownRenderer>;
}
