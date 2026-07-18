interface Item<T extends string> { value: T; label: string; disabled?: boolean; title?: string; }

export function SegmentedControl<T extends string>({ label, value, items, onChange }: { label: string; value: T; items: Item<T>[]; onChange(value: T): void }) {
  return <div className="segmented" role="group" aria-label={label}>{items.map((item) => <button key={item.value} type="button" className={value === item.value ? "selected" : ""} aria-pressed={value === item.value} disabled={item.disabled} title={item.title} onClick={() => onChange(item.value)}>{item.label}</button>)}</div>;
}
