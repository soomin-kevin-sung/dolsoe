import { useState } from "react";

export function OptionRow({ label, flag, initial, min, max, value: controlledValue, onValueChange }: { label: string; flag: string; initial: number; min: number; max: number; value?: number; onValueChange?(value: number): void }) {
  const [localValue, setLocalValue] = useState(String(initial));
  const [notice, setNotice] = useState(false);
  const value = controlledValue === undefined ? localValue : String(controlledValue);
  function update(next: string) { if (controlledValue === undefined) setLocalValue(next); else { const parsed = Number(next); if (Number.isFinite(parsed)) onValueChange?.(parsed); } }
  return <div className="option-wrap"><label className="option-row"><span><strong>{label}</strong><code>{flag}</code></span><input type="number" aria-label={label} value={value} min={min} max={max} onChange={(event) => update(event.target.value)} onBlur={() => { const parsed = Number(value); const clamped = Number.isFinite(parsed) ? Math.min(max, Math.max(min, parsed)) : initial; setNotice(clamped !== parsed); if (controlledValue === undefined) setLocalValue(String(clamped)); else onValueChange?.(clamped); }} /></label>{notice && <span className="clamp-note">허용 범위에 맞게 조정했습니다.</span>}</div>;
}
