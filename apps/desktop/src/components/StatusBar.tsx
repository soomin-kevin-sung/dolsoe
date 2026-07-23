import type { RuntimeSnapshot } from "../services/runtime";

export function StatusBar({ snapshot, compact = false }: { snapshot: RuntimeSnapshot; compact?: boolean }) {
  const metrics = [["백엔드", snapshot.telemetry.backend], ["속도", snapshot.telemetry.speed], ["토큰", snapshot.telemetry.tokens], ["컨텍스트", snapshot.telemetry.context], ["시간", snapshot.telemetry.elapsed]];
  return <footer className={`status-bar ${compact ? "compact" : ""}`} role="status"><div className="status-left"><span className={`status-dot ${snapshot.runtimeStatus}`} /><span className="status-text">{snapshot.statusText}</span><span className="status-model" data-model-name>{snapshot.modelName}</span></div>{!compact && <div className="status-metrics">{metrics.map(([label, value]) => <div className={`status-metric metric-${label}`} key={label}><span className="status-metric-label">{label}</span><span className={`status-metric-value ${snapshot.runtimeStatus === "streaming" && label === "속도" ? "live" : ""}`}>{value || "—"}</span></div>)}</div>}</footer>;
}
