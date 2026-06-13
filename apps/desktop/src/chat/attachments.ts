import type { AppContentBlock } from "../generated/contracts";

export type PromptAttachment = {
  path: string;
  name: string;
  mimeType: string | null;
  kind: "file" | "image" | "audio" | "video";
};

export type PromptSubmission = {
  text: string;
  attachments: PromptAttachment[];
};

export function pathsToAttachments(paths: string[]): PromptAttachment[] {
  return paths.map((path) => {
    const name = path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
    const mimeType = mimeTypeForPath(path);
    return {
      path,
      name,
      mimeType,
      kind: mediaKindForMimeType(mimeType),
    };
  });
}

export function submissionContentBlocks(submission: PromptSubmission): AppContentBlock[] {
  const blocks: AppContentBlock[] = [];
  const text = submission.text.trimEnd();
  if (text.trim().length > 0) {
    blocks.push({ type: "text", text });
  }
  for (const attachment of submission.attachments) {
    blocks.push({
      type: "media",
      media: {
        kind: attachment.kind,
        source: { type: "uri", uri: pathToFileUri(attachment.path) },
        mimeType: attachment.mimeType,
        name: attachment.name,
        metadata: {},
      },
    });
  }
  return blocks;
}

export function optimisticPromptText(submission: PromptSubmission): string {
  const text = submission.text.trimEnd();
  const names = submission.attachments.map((attachment) => attachment.name);
  if (names.length === 0) {
    return text;
  }
  const attachmentLine = names.map((name) => `@${name}`).join(" ");
  return text.trim().length > 0 ? `${text}\n\n${attachmentLine}` : attachmentLine;
}

export function pathToFileUri(path: string): string {
  if (path.startsWith("file://")) {
    return path;
  }
  const encoded = path.split("/").map(encodeURIComponent).join("/");
  return `file://${encoded}`;
}

function mediaKindForMimeType(mimeType: string | null): PromptAttachment["kind"] {
  if (!mimeType) {
    return "file";
  }
  if (mimeType.startsWith("image/")) {
    return "image";
  }
  if (mimeType.startsWith("audio/")) {
    return "audio";
  }
  if (mimeType.startsWith("video/")) {
    return "video";
  }
  return "file";
}

function mimeTypeForPath(path: string): string | null {
  const extension = path.split(".").at(-1)?.toLowerCase();
  if (!extension) {
    return null;
  }
  return (
    {
      avif: "image/avif",
      gif: "image/gif",
      jpg: "image/jpeg",
      jpeg: "image/jpeg",
      png: "image/png",
      webp: "image/webp",
      mp3: "audio/mpeg",
      m4a: "audio/mp4",
      wav: "audio/wav",
      mp4: "video/mp4",
      mov: "video/quicktime",
      pdf: "application/pdf",
      txt: "text/plain",
      md: "text/markdown",
      json: "application/json",
      jsonc: "application/json",
    } satisfies Record<string, string>
  )[extension] ?? null;
}
