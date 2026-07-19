import { Check, Cpu, Download, MonitorCog, Wrench, XCircle } from "lucide-react";

import type { AvailableRuntimePack, RuntimeBackend, RuntimeInstallState, RuntimePack } from "../services/runtimePacks";

const labels: Record<RuntimeBackend, string> = { cpu: "CPU", cuda: "CUDA", vulkan: "Vulkan" };

const statusLabels = {
  ready: "사용 가능",
  "not-installed": "설치 안 됨",
  "replacement-pending": "재시작 필요",
  "repair-required": "복구 필요",
  unavailable: "장비 또는 드라이버 없음",
} as const;

function formatBytes(value: number) {
  return value >= 1024 ** 3
    ? `${(value / 1024 ** 3).toFixed(1)} GB`
    : `${Math.max(1, Math.round(value / 1024 ** 2))} MB`;
}

interface Props {
  backend: RuntimeBackend;
  pack: RuntimePack;
  available?: AvailableRuntimePack;
  active: boolean;
  selected: boolean;
  installState: RuntimeInstallState | null;
  onSelect(): void;
  onInstall(): void;
  onCancel(): void;
}

export function RuntimeBackendRow({ backend, pack, available, active, selected, installState, onSelect, onInstall, onCancel }: Props) {
  const installing = installState?.packId === backend
    && ["downloading", "verifying", "installing"].includes(installState.phase);
  const actionable = pack.status === "ready";
  const Icon = backend === "cpu" ? Cpu : MonitorCog;
  return (
    <div className={`runtime-backend-row ${selected ? "selected" : ""}`}>
      <button className="runtime-backend-main" type="button" onClick={actionable ? onSelect : onInstall} disabled={pack.status === "unavailable" || installing}>
        <Icon size={18} aria-hidden="true" />
        <span className="runtime-backend-copy">
          <strong>{labels[backend]}</strong>
          <small>{statusLabels[pack.status]}</small>
        </span>
        {active && <span className="runtime-active-mark"><Check size={13} /> 사용 중</span>}
      </button>
      <div className="runtime-backend-meta">
        <span>{pack.runtimeVersion ?? available?.releaseVersion ?? "-"}</span>
        {available && <span>{formatBytes(available.sizeBytes)}</span>}
        {pack.devices[0] && <span>{pack.devices[0].name}</span>}
      </div>
      {pack.error && <p className="runtime-pack-error"><XCircle size={13} /> {pack.error}</p>}
      {installing && <div className="runtime-install-progress">
        <div className="progress-track"><div className="progress-fill" style={{ width: `${installState.progress}%` }} /></div>
        <span>{installState.progress}%</span>
        {installState.phase === "downloading" && <button className="button-secondary" type="button" onClick={onCancel}>취소</button>}
      </div>}
      {!installing && (pack.status === "not-installed" || pack.status === "repair-required") && (
        <button className="runtime-row-action" type="button" onClick={onInstall} disabled={!available}>
          {pack.status === "repair-required" ? <Wrench size={14} /> : <Download size={14} />}
          {pack.status === "repair-required" ? "복구" : "설치"}
        </button>
      )}
    </div>
  );
}
