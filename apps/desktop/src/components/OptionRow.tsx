import { useEffect, useState } from "react";

import { describeOption } from "../services/optionPresentation";

export function OptionRow({ label, flag, initial, min, max, step = 1, value: controlledValue, onValueChange }: { label: string; flag: string; initial: number; min: number; max: number; step?: number; value?: number; onValueChange?(value: number): void }) {
  const [draft, setDraft] = useState(String(controlledValue ?? initial));
  const [notice, setNotice] = useState(false);

  useEffect(() => {
    setDraft(String(controlledValue ?? initial));
  }, [controlledValue, initial]);

  function commit() {
    const parsed = Number(draft);
    const clamped = Number.isFinite(parsed) ? Math.min(max, Math.max(min, parsed)) : initial;
    setNotice(clamped !== parsed);
    setDraft(String(clamped));
    onValueChange?.(clamped);
  }

  const parsedDraft = draft.trim() === "" ? Number.NaN : Number(draft);
  const presentation = Number.isFinite(parsedDraft) ? describeOption(flag, parsedDraft) : null;

  return <div className="option-wrap">
    <label className="option-row">
      <span className="option-copy"><strong>{label}</strong><small className="option-description">{presentation?.description ?? "숫자 값을 입력하세요."}</small></span>
      <code className="option-flag">{flag}</code>
      <span className="option-value-stack">
        <input type="number" aria-label={label} value={draft} min={min} max={max} step={step} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") event.currentTarget.blur(); }} onBlur={commit} />
        <small className="option-effect">{presentation?.effect ?? "현재 값을 확인할 수 없습니다."}</small>
      </span>
    </label>
    {notice && <span className="clamp-note">허용 범위에 맞게 조정했습니다.</span>}
  </div>;
}
