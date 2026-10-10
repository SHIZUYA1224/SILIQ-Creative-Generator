import { useRef, useState, type ReactNode } from 'react';

export function Field({ label, children }: { label: string; children: ReactNode }) {
  return <label className="field"><span>{label}</span>{children}</label>;
}
export function NumberField({ label, value, min, max, step = 1, disabled, onCommit }: {
  label: string; value: number; min?: number; max?: number; step?: number; disabled?: boolean; onCommit: (value: number) => void;
}) {
  const [draft, setDraft] = useState(String(value));
  return <Field label={label}><input aria-label={label} type="number" value={draft} min={min} max={max} step={step} disabled={disabled}
    onChange={e => setDraft(e.target.value)} onKeyDown={e => { if (e.key === 'Enter') e.currentTarget.blur(); }}
    onBlur={() => { const n = Number(draft); if (draft !== '' && Number.isFinite(n) && (min === undefined || n >= min) && (max === undefined || n <= max)) { if (n !== value) onCommit(n); } else setDraft(String(value)); }} /></Field>;
}
export function TextField({ label, value, multiline = false, disabled, onCommit }: {
  label: string; value: string; multiline?: boolean; disabled?: boolean; onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  const composing = useRef(false);
  const pendingBlur = useRef(false);
  const commit = (text: string) => { if (text !== value) onCommit(text); };
  const common = { 'aria-label': label, value: draft, disabled,
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => setDraft(e.target.value),
    onCompositionStart: () => { composing.current = true; },
    onCompositionEnd: (e: React.CompositionEvent<HTMLInputElement | HTMLTextAreaElement>) => { composing.current = false; setDraft(e.currentTarget.value); if (pendingBlur.current) { pendingBlur.current = false; commit(e.currentTarget.value); } },
    onBlur: (e: React.FocusEvent<HTMLInputElement | HTMLTextAreaElement>) => { if (composing.current) pendingBlur.current = true; else commit(e.currentTarget.value); },
  };
  return <Field label={label}>{multiline ? <textarea {...common} rows={4} /> : <input {...common} onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) e.currentTarget.blur(); }} />}</Field>;
}
export function Section({ title, children }: { title: string; children: ReactNode }) {
  return <section className="property-section"><h3>{title}</h3>{children}</section>;
}
