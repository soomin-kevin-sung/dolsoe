import { useState } from "react";

export function OptionRow({ label, flag, initial, min, max }: { label: string; flag: string; initial: number; min: number; max: number }) {
  const [value, setValue] = useState(String(initial));
  const [notice, setNotice] = useState(false);
  return <div className="option-wrap"><label className="option-row"><span><strong>{label}</strong><code>{flag}</code></span><input type="number" aria-label={label} value={value} min={min} max={max} onChange={(event) => setValue(event.target.value)} onBlur={() => { const parsed = Number(value); const clamped = Number.isFinite(parsed) ? Math.min(max, Math.max(min, parsed)) : initial; setNotice(clamped !== parsed); setValue(String(clamped)); }} /></label>{notice && <span className="clamp-note">허용 범위에 맞게 조정했습니다.</span>}</div>;
}
