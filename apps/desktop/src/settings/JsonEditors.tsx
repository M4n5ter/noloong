import { Plus, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";

export function JsonObjectEditor<T>({
  fallback,
  label,
  onChange,
  value,
}: {
  fallback: T;
  label: string;
  value: T | null;
  onChange: (value: T | null) => void;
}) {
  const [text, setText] = useState(JSON.stringify(value ?? fallback, null, 2));
  const [error, setError] = useState<string | null>(null);
  const serializedValue = JSON.stringify(value ?? fallback, null, 2);
  useEffect(() => {
    setText(serializedValue);
  }, [serializedValue]);
  return (
    <label className="json-field">
      <span>{label}</span>
      <textarea
        onBlur={() => {
          try {
            onChange(JSON.parse(text) as T);
            setError(null);
          } catch (parseError) {
            setError(String(parseError));
          }
        }}
        onChange={(event) => setText(event.target.value)}
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
  onUpsert,
}: {
  addLabel: string;
  deleteLabel: string;
  emptyLabel: string;
  fallback: T;
  items: T[];
  onDelete: (index: number) => void;
  onUpsert: (item: T, index: number | null) => void;
}) {
  return (
    <div className="json-list">
      <button className="text-button icon-text" onClick={() => onUpsert(fallback, null)} type="button">
        <Plus size={15} />
        <span>{addLabel}</span>
      </button>
      {items.length === 0 ? <p className="muted">{emptyLabel}</p> : null}
      {items.map((item, index) => (
        <div className="json-list-item" key={index}>
          <JsonObjectEditor
            fallback={fallback}
            label={`#${index + 1}`}
            onChange={(value) => value && onUpsert(value, index)}
            value={item}
          />
          <button className="text-button danger icon-text" onClick={() => onDelete(index)} type="button">
            <Trash2 size={15} />
            <span>{deleteLabel}</span>
          </button>
        </div>
      ))}
    </div>
  );
}
