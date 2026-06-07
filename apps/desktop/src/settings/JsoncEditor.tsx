import { autocompletion, type CompletionContext } from "@codemirror/autocomplete";
import { json } from "@codemirror/lang-json";
import { Compartment } from "@codemirror/state";
import { basicSetup, EditorView } from "codemirror";
import { useEffect, useRef } from "react";
import type { AppProfileConfigCompletionSet } from "../generated/contracts";

export type JsoncEditorProps = {
  value: string;
  readOnly?: boolean;
  complete(text: string, byteOffset: number): Promise<AppProfileConfigCompletionSet>;
  onChange(value: string): void;
};

export function JsoncEditor({ value, readOnly = false, complete, onChange }: JsoncEditorProps) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const viewRef = useRef<EditorView | null>(null);
  const editableCompartmentRef = useRef(new Compartment());
  const completeRef = useRef(complete);
  const onChangeRef = useRef(onChange);

  useEffect(() => {
    completeRef.current = complete;
    onChangeRef.current = onChange;
  }, [complete, onChange]);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) {
      return;
    }
    const view = new EditorView({
      doc: value,
      parent: container,
      extensions: [
        basicSetup,
        json(),
        EditorView.lineWrapping,
        editableCompartmentRef.current.of(EditorView.editable.of(!readOnly)),
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            onChangeRef.current(update.state.doc.toString());
          }
        }),
        autocompletion({
          activateOnTyping: true,
          override: [profileConfigCompletionSource],
        }),
        EditorView.theme({
          "&": {
            height: "100%",
            backgroundColor: "transparent",
            color: "#e5edf7",
            fontSize: "13px",
          },
          ".cm-scroller": {
            fontFamily:
              "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
            lineHeight: "1.65",
          },
          ".cm-gutters": {
            backgroundColor: "rgba(3, 8, 14, 0.52)",
            color: "rgba(220, 230, 243, 0.42)",
            borderRight: "1px solid rgba(139, 162, 189, 0.12)",
          },
          ".cm-activeLine": {
            backgroundColor: "rgba(113, 160, 255, 0.08)",
          },
          ".cm-activeLineGutter": {
            backgroundColor: "rgba(113, 160, 255, 0.08)",
          },
          ".cm-tooltip": {
            backgroundColor: "#070d14",
            border: "1px solid rgba(139, 162, 189, 0.24)",
            borderRadius: "12px",
            overflow: "hidden",
          },
          ".cm-tooltip-autocomplete ul": {
            fontFamily: "inherit",
          },
        }),
      ],
    });
    viewRef.current = view;
    return () => {
      view.destroy();
      viewRef.current = null;
    };
  }, []);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) {
      return;
    }
    const current = view.state.doc.toString();
    if (current === value) {
      return;
    }
    view.dispatch({
      changes: {
        from: 0,
        to: current.length,
        insert: value,
      },
    });
  }, [value]);

  useEffect(() => {
    const view = viewRef.current;
    if (!view) {
      return;
    }
    view.dispatch({
      effects: editableCompartmentRef.current.reconfigure(EditorView.editable.of(!readOnly)),
    });
  }, [readOnly]);

  async function profileConfigCompletionSource(context: CompletionContext) {
    if (!context.explicit && !completionTriggerBefore(context)) {
      return null;
    }
    const text = context.state.doc.toString();
    const byteOffset = utf8ByteOffset(text, context.pos);
    const completionSet = await completeRef.current(text, byteOffset);
    if (completionSet.completions.length === 0) {
      return null;
    }
    return {
      from: codeUnitOffsetFromUtf8ByteOffset(text, completionSet.replaceStart),
      options: completionSet.completions.map((completion) => ({
        label: completion.label,
        apply: completion.insertText,
        detail: completion.detail ?? undefined,
        info: completion.documentation ?? undefined,
        type: completion.kind,
      })),
    };
  }

  return <div className="jsonc-editor" ref={containerRef} />;
}

function completionTriggerBefore(context: CompletionContext): boolean {
  const before = context.matchBefore(/[A-Za-z0-9_"-]*$/);
  if (before && before.from < context.pos) {
    return true;
  }
  const previous = context.state.sliceDoc(Math.max(0, context.pos - 1), context.pos);
  return previous === '"' || previous === ":" || previous === "," || previous === "{" || previous === "[";
}

function utf8ByteOffset(text: string, codeUnitOffset: number): number {
  return new TextEncoder().encode(text.slice(0, codeUnitOffset)).length;
}

function codeUnitOffsetFromUtf8ByteOffset(text: string, byteOffset: number): number {
  let bytes = 0;
  let codeUnits = 0;
  for (const char of text) {
    const nextBytes = bytes + new TextEncoder().encode(char).length;
    if (nextBytes > byteOffset) {
      return codeUnits;
    }
    bytes = nextBytes;
    codeUnits += char.length;
  }
  return text.length;
}
