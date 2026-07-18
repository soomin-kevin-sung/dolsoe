import type { NativeState } from "../services/nativeState";

export function NativeDiagnosticsView({ state }: { state: NativeState }) {
  const rows = [
    ["상태", state.phase],
    ["백엔드", state.backend],
    ["모델", state.modelName],
    ["속도", state.telemetry.speed],
    ["토큰", state.telemetry.tokens],
    ["시간", state.telemetry.elapsed],
    ["요청 핸들", state.activeRequestHandle ?? "—"],
  ];
  return <div className="diagnostics"><h1>진단</h1><section className="diagnostic-section"><h2>현재 로컬 추론</h2>{rows.map(([label, value]) => <div className="diagnostic-row" key={label}><span>{label}</span><code>{value}</code></div>)}</section></div>;
}
