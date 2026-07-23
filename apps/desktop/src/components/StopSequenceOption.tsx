import { Plus, X } from "lucide-react";
import { useState, type KeyboardEvent } from "react";

interface Props {
  values: string[];
  onChange(values: string[]): void;
}

const MAX_STOP_SEQUENCES = 16;

export function StopSequenceOption({ values, onChange }: Props) {
  const [draft, setDraft] = useState("");

  function add() {
    const value = draft.trim();
    if (!value || values.includes(value) || values.length >= MAX_STOP_SEQUENCES) return;
    onChange([...values, value]);
    setDraft("");
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      add();
    }
  }

  return (
    <div className="stop-option">
      <div className="stop-option-heading"><span><strong>중지 문자열</strong><code>stop[]</code></span><small>{values.length}/{MAX_STOP_SEQUENCES}</small></div>
      {values.length > 0 && <div className="stop-sequence-list">{values.map((value) => (
        <span className="stop-sequence" key={value} title={value}><code>{value}</code><button type="button" aria-label={`${value} 제거`} title="중지 문자열 제거" onClick={() => onChange(values.filter((candidate) => candidate !== value))}><X size={12} /></button></span>
      ))}</div>}
      <div className="stop-option-entry">
        <input type="text" aria-label="중지 문자열" maxLength={256} value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={onKeyDown} />
        <button className="icon-button" type="button" aria-label="중지 문자열 추가" title="중지 문자열 추가" disabled={!draft.trim() || values.includes(draft.trim()) || values.length >= MAX_STOP_SEQUENCES} onClick={add}><Plus size={15} /></button>
      </div>
    </div>
  );
}
