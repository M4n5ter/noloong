import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { Maximize2, Minimize2, Paperclip, Send, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { AppI18n } from "../i18n";
import { pathsToAttachments, type PromptAttachment, type PromptSubmission } from "./attachments";

const COMPACT_TEXT_LIMIT = 96;

export function PromptComposer({
  disabled,
  i18n,
  onSubmit,
  placeholder,
}: {
  disabled: boolean;
  i18n: AppI18n;
  onSubmit: (submission: PromptSubmission) => Promise<void>;
  placeholder: string;
}) {
  const [text, setText] = useState("");
  const [attachments, setAttachments] = useState<PromptAttachment[]>([]);
  const [dragging, setDragging] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const [scrollFades, setScrollFades] = useState({ top: false, bottom: false });
  const formRef = useRef<HTMLFormElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const disabledRef = useRef(disabled);
  const canSend = (text.trim().length > 0 || attachments.length > 0) && !disabled;
  const canExpand = expanded || needsExpandedComposer(text);
  const previewingCompactOverflow = canExpand && !expanded;
  const compactPreview = firstPreviewLine(text);

  useEffect(() => {
    disabledRef.current = disabled;
    if (disabled) {
      setDragging(false);
    }
  }, [disabled]);

  useEffect(() => {
    if (expanded) {
      textareaRef.current?.focus();
    }
  }, [expanded]);

  const updateScrollFades = useCallback(() => {
    const textarea = textareaRef.current;
    if (!expanded || !textarea) {
      setScrollFades({ top: false, bottom: false });
      return;
    }
    setScrollFades(composerScrollFadeState(textarea));
  }, [expanded]);

  useLayoutEffect(() => {
    updateScrollFades();
  }, [expanded, text, updateScrollFades]);

  const submit = useCallback(async () => {
    if (!canSend) {
      return;
    }
    const submitted = text;
    const submittedAttachments = attachments;
    setText("");
    setAttachments([]);
    setExpanded(false);
    await onSubmit({ text: submitted, attachments: submittedAttachments });
  }, [attachments, canSend, onSubmit, text]);

  const addPaths = useCallback((paths: string[]) => {
    if (disabledRef.current) {
      return;
    }
    setAttachments((current) => {
      const existing = new Set(current.map((attachment) => attachment.path));
      return [
        ...current,
        ...pathsToAttachments(paths).filter((attachment) => !existing.has(attachment.path)),
      ];
    });
  }, []);

  const pickFiles = useCallback(async () => {
    if (disabledRef.current) {
      return;
    }
    const selected = await open({ multiple: true, directory: false });
    if (!selected || disabledRef.current) {
      return;
    }
    addPaths(Array.isArray(selected) ? selected : [selected]);
  }, [addPaths]);

  const isComposerDropPosition = useCallback((position: { x: number; y: number }) => {
    const form = formRef.current;
    if (!form) {
      return false;
    }
    const scale = window.devicePixelRatio || 1;
    const x = position.x / scale;
    const y = position.y / scale;
    const rect = form.getBoundingClientRect();
    return x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom;
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    try {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          if (disabledRef.current) {
            setDragging(false);
            return;
          }
          switch (event.payload.type) {
            case "enter":
            case "over":
              setDragging(isComposerDropPosition(event.payload.position));
              break;
            case "drop":
              setDragging(false);
              if (isComposerDropPosition(event.payload.position)) {
                addPaths(event.payload.paths);
              }
              break;
            case "leave":
              setDragging(false);
              break;
          }
        })
        .then((dispose) => {
          if (!active) {
            dispose();
            return;
          }
          unlisten = dispose;
        })
        .catch(() => {
          unlisten = null;
        });
    } catch {
      unlisten = null;
    }
    return () => {
      active = false;
      unlisten?.();
    };
  }, [addPaths, isComposerDropPosition]);

  return (
    <form
      className={["composer", dragging ? "dragging" : ""].filter(Boolean).join(" ")}
      onSubmit={(event) => {
        event.preventDefault();
        void submit();
      }}
      ref={formRef}
    >
      <div className="composer-capsule" onClick={() => textareaRef.current?.focus()}>
        <div className="composer-input-shell">
          {expanded || previewingCompactOverflow ? (
            <span aria-hidden="true" className="composer-preview">
              {compactPreview || placeholder}
            </span>
          ) : null}
          <div
            className={[
              "composer-editor-shell",
              expanded ? "expanded" : "",
              previewingCompactOverflow ? "previewing" : "",
              scrollFades.top ? "fade-top" : "",
              scrollFades.bottom ? "fade-bottom" : "",
            ]
              .filter(Boolean)
              .join(" ")}
          >
            <textarea
              aria-label={i18n.t("composer.write")}
              className="composer-input"
              disabled={disabled}
              onChange={(event) => setText(event.target.value)}
              onKeyDown={(event) => {
                if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                  event.preventDefault();
                  void submit();
                }
              }}
              onScroll={updateScrollFades}
              placeholder={placeholder}
              ref={textareaRef}
              rows={expanded ? 6 : 1}
              value={text}
            />
          </div>
        </div>
        {canExpand ? (
          <button
            aria-label={i18n.t(expanded ? "composer.collapse" : "composer.expand")}
            className="composer-expand"
            onClick={(event) => {
              event.preventDefault();
              event.stopPropagation();
              setExpanded((current) => !current);
            }}
            type="button"
          >
            {expanded ? <Minimize2 size={12} /> : <Maximize2 size={12} />}
          </button>
        ) : null}
        <div className="composer-actions">
          <button
            aria-label={i18n.t("composer.attach")}
            className="composer-tool"
            disabled={disabled}
            onClick={(event) => {
              event.stopPropagation();
              void pickFiles();
            }}
            type="button"
          >
            <Paperclip size={16} />
          </button>
          <button
            aria-label={i18n.t("composer.send")}
            className="send-button"
            disabled={!canSend}
            type="submit"
          >
            <Send size={16} />
          </button>
        </div>
      </div>
      {attachments.length > 0 ? (
        <div className="attachment-strip">
          {attachments.map((attachment) => (
            <span className="attachment-pill" key={attachment.path} title={attachment.path}>
              <span className="attachment-name">{attachment.name}</span>
              <button
                aria-label={i18n.t("composer.removeAttachment", { name: attachment.name })}
                onClick={(event) => {
                  event.preventDefault();
                  event.stopPropagation();
                  setAttachments((current) =>
                    current.filter((item) => item.path !== attachment.path),
                  );
                }}
                type="button"
              >
                <X size={12} />
              </button>
            </span>
          ))}
        </div>
      ) : null}
    </form>
  );
}

function firstPreviewLine(text: string): string {
  return text.split(/\r?\n/, 1)[0]?.trimEnd() ?? "";
}

function needsExpandedComposer(text: string): boolean {
  return text.includes("\n") || text.length > COMPACT_TEXT_LIMIT;
}

function composerScrollFadeState(metrics: {
  clientHeight: number;
  scrollHeight: number;
  scrollTop: number;
}): { top: boolean; bottom: boolean } {
  const overflow = metrics.scrollHeight - metrics.clientHeight;
  if (overflow <= 1) {
    return { top: false, bottom: false };
  }
  return {
    top: metrics.scrollTop > 1,
    bottom: overflow - metrics.scrollTop > 1,
  };
}
