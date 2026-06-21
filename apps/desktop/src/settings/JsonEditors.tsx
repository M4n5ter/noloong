import { Plus, Trash2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

let jsonEditorKeyCounter = 0;

export function JsonObjectEditor<T>({
  fallback,
  label,
  errorKey = label,
  onChange,
  onParseErrorChange,
  value,
}: {
  errorKey?: string;
  fallback: T;
  label: string;
  value: T | null;
  onChange: (value: T | null) => void;
  onParseErrorChange?: (key: string, error: string | null) => void;
}) {
  const [text, setText] = useState(JSON.stringify(value ?? fallback, null, 2));
  const [error, setError] = useState<string | null>(null);
  const serializedValue = JSON.stringify(value ?? fallback, null, 2);
  const focusedRef = useRef(false);
  const lastSerializedValueRef = useRef(serializedValue);

  useEffect(() => {
    if (serializedValue === lastSerializedValueRef.current) {
      return;
    }
    lastSerializedValueRef.current = serializedValue;
    if (!focusedRef.current) {
      setText(serializedValue);
      setError(null);
      onParseErrorChange?.(errorKey, null);
    }
  }, [errorKey, onParseErrorChange, serializedValue]);

  useEffect(() => {
    return () => {
      onParseErrorChange?.(errorKey, null);
    };
  }, [errorKey, onParseErrorChange]);

  function parseText(nextText: string): T | null {
    return JSON.parse(nextText) as T | null;
  }

  function syncValidText(nextText: string): boolean {
    try {
      onChange(parseText(nextText));
      setError(null);
      onParseErrorChange?.(errorKey, null);
      return true;
    } catch (parseError) {
      const message = String(parseError);
      setError(message);
      onParseErrorChange?.(errorKey, message);
      return false;
    }
  }

  return (
    <label className="json-field">
      <span>{label}</span>
      <textarea
        onBlur={() => {
          focusedRef.current = false;
          try {
            const parsed = parseText(text);
            onChange(parsed);
            setText(JSON.stringify(parsed ?? fallback, null, 2));
            setError(null);
            onParseErrorChange?.(errorKey, null);
          } catch (parseError) {
            const message = String(parseError);
            setError(message);
            onParseErrorChange?.(errorKey, message);
          }
        }}
        onChange={(event) => {
          const nextText = event.target.value;
          setText(nextText);
          syncValidText(nextText);
        }}
        onFocus={() => {
          focusedRef.current = true;
        }}
        rows={6}
        value={text}
      />
      {error ? <small>{error}</small> : null}
    </label>
  );
}

export function JsonListEditor<T>({
  addLabel,
  deleteLabel,
  emptyLabel,
  fallback,
  items,
  onDelete,
  onParseErrorChange,
  onUpsert,
}: {
  addLabel: string;
  deleteLabel: string;
  emptyLabel: string;
  fallback: T;
  items: T[];
  onDelete: (index: number) => void;
  onParseErrorChange?: (key: string, error: string | null) => void;
  onUpsert: (item: T, index: number | null) => void;
}) {
  const [itemKeys, setItemKeys] = useState<string[]>(() => items.map(() => nextJsonEditorKey()));

  useEffect(() => {
    setItemKeys((current) => {
      if (current.length === items.length) {
        return current;
      }
      if (current.length > items.length) {
        return current.slice(0, items.length);
      }
      return [
        ...current,
        ...Array.from({ length: items.length - current.length }, nextJsonEditorKey),
      ];
    });
  }, [items.length]);

  const addButton = (
    <button
      className="text-button icon-text"
      onClick={() => {
        setItemKeys((current) => [...current, nextJsonEditorKey()]);
        onUpsert(fallback, null);
      }}
      type="button"
    >
      <Plus size={15} />
      <span>{addLabel}</span>
    </button>
  );

  return (
    <div className="json-list">
      {items.length === 0 ? (
        <div className="json-list-empty">
          <p className="muted">{emptyLabel}</p>
          {addButton}
        </div>
      ) : (
        addButton
      )}
      {items.map((item, index) => (
        <div className="json-list-item" key={itemKeys[index] ?? index}>
          <JsonObjectEditor
            errorKey={`json-list-${itemKeys[index] ?? index}`}
            fallback={fallback}
            label={`#${index + 1}`}
            onChange={(value) => value && onUpsert(value, index)}
            onParseErrorChange={onParseErrorChange}
            value={item}
          />
          <button
            className="text-button danger icon-text"
            onClick={() => {
              onParseErrorChange?.(`json-list-${itemKeys[index] ?? index}`, null);
              setItemKeys((current) => current.filter((_, itemIndex) => itemIndex !== index));
              onDelete(index);
            }}
            type="button"
          >
            <Trash2 size={15} />
            <span>{deleteLabel}</span>
          </button>
        </div>
      ))}
    </div>
  );
}

function nextJsonEditorKey(): string {
  jsonEditorKeyCounter += 1;
  return `json-editor-${jsonEditorKeyCounter}`;
}
