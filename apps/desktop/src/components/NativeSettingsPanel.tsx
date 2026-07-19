import { Cpu, X } from "lucide-react";

import type { NativeOptions } from "../hooks/useNativeRuntime";
import type { AvailableRuntimePack, RuntimeBackend, RuntimeInstallState, RuntimePack, RuntimeSelection } from "../services/runtimePacks";
import { IconButton } from "./IconButton";
import { OptionRow } from "./OptionRow";
import { SegmentedControl } from "./SegmentedControl";

interface Props {
  open: boolean;
  modelName: string;
  options: NativeOptions;
  runtimePacks: RuntimePack[];
  runtimePackError: string | null;
  availableRuntimePacks: AvailableRuntimePack[];
  installState: RuntimeInstallState | null;
  distributionError: string | null;
  appliedRuntime: RuntimeSelection | null;
  pendingRuntime: RuntimeSelection | null;
  onOptionsChange(options: NativeOptions): void;
  onRuntimeChange(backend: RuntimeBackend): void;
  onApplyRuntime(): void;
  onClose(): void;
  onChooseModel(): void;
  onUnload(): void;
  onReload(): void;
  onInstall(packId: string): void;
  onCancelInstall(): void;
}

const backends: { value: RuntimeBackend; label: string }[] = [
  { value: "cpu", label: "CPU" },
  { value: "cuda", label: "CUDA" },
  { value: "vulkan", label: "Vulkan" },
];

function formatBytes(value: number) {
  return value >= 1024 ** 3 ? `${(value / 1024 ** 3).toFixed(1)} GB` : `${Math.max(1, Math.round(value / 1024 ** 2))} MB`;
}

const phaseLabels = { downloading: "다운로드 중", verifying: "검증 중", installing: "설치 중", installed: "설치 완료", cancelled: "취소됨", failed: "설치 실패" } as const;

export function NativeSettingsPanel({ open, modelName, options, runtimePacks, runtimePackError, availableRuntimePacks, installState, distributionError, appliedRuntime, pendingRuntime, onOptionsChange, onRuntimeChange, onApplyRuntime, onClose, onChooseModel, onUnload, onReload, onInstall, onCancelInstall }: Props) {
  const set = (key: keyof NativeOptions) => (value: number) => onOptionsChange({ ...options, [key]: value });
  const selected = pendingRuntime ?? appliedRuntime;
  const selectedPack = runtimePacks.find((pack) => pack.id === selected?.packId);
  const selectedDevice = selectedPack?.devices.find((device) => device.index === selected?.deviceIndex);
  const runtimeChanged = Boolean(appliedRuntime && pendingRuntime
    && (appliedRuntime.packId !== pendingRuntime.packId || appliedRuntime.backend !== pendingRuntime.backend));
  return <aside className={`settings-panel ${open ? "open" : ""}`} aria-label="설정" hidden={!open}>
    <div className="panel-header"><h2>설정</h2><IconButton icon={X} label="설정 닫기" onClick={onClose} /></div>
    <div className="panel-body">
      <section className="settings-section"><h3>모델</h3><strong className="model-file">{modelName}</strong><p>로컬 GGUF · 메모리 내 대화</p><div className="panel-actions"><button className="button-secondary" type="button" onClick={onChooseModel}>다른 모델 선택…</button><button className="button-secondary" type="button" onClick={onUnload}>모델 언로드</button></div></section>
      <section className="settings-section"><h3>런타임 {runtimeChanged && <span className="reload-badge">재로드 대기</span>}</h3><SegmentedControl<RuntimeBackend> label="런타임" value={selected?.backend ?? "cpu"} onChange={onRuntimeChange} items={backends.map((item) => ({ ...item, disabled: !runtimePacks.some((pack) => pack.status === "ready" && pack.backend === item.value), title: runtimePacks.some((pack) => pack.status === "ready" && pack.backend === item.value) ? undefined : `${item.label} 런타임이 설치되어 있지 않습니다.` }))} />
        {selectedPack && <p className="device-line"><Cpu size={14} /> {selectedDevice?.name ?? selectedPack.id} · {selectedPack.id}</p>}
        {backends.filter((item) => !runtimePacks.some((pack) => pack.status === "ready" && pack.backend === item.value)).map((item) => <p className="runtime-unavailable" key={item.value}>{item.label} 런타임이 설치되어 있지 않습니다.</p>)}
        {runtimePackError && <p className="runtime-pack-error">런타임 팩을 확인하지 못했습니다: {runtimePackError}</p>}
        {availableRuntimePacks.map((pack) => {
          const active = installState?.packId === pack.id ? installState : null;
          const busy = Boolean(installState && !["installed", "cancelled", "failed"].includes(installState.phase));
          return <div className="pack-row" key={pack.id}>
            <div className="pack-heading"><strong>{pack.backend.toUpperCase()} 런타임 팩</strong><span className="mono">{formatBytes(pack.sizeBytes)}</span>
              {!pack.installed && !active && <button className="button-secondary" type="button" disabled={busy} onClick={() => onInstall(pack.id)}>설치</button>}
              {active && active.phase === "downloading" && <button className="button-secondary" type="button" onClick={onCancelInstall}>취소</button>}
            </div>
            <div className="pack-version mono">{pack.releaseVersion} · {pack.id}</div>
            {active && <><div className="progress-track"><div className="progress-fill" style={{ width: `${active.progress}%` }} /></div><div className="pack-status">{phaseLabels[active.phase]} | {active.progress}%</div></>}
            {active?.error && <p className="runtime-pack-error">{active.error}</p>}
            {(pack.installed || active?.phase === "installed") && <p className="runtime-installed">앱을 재시작하면 사용할 수 있습니다.</p>}
          </div>;
        })}
        {distributionError && <p className="runtime-pack-error">온라인 런타임 팩을 확인하지 못했습니다: {distributionError}</p>}
      </section>
      <section className="settings-section"><h3>추론 옵션</h3>
        <OptionRow label="컨텍스트 길이" flag="--ctx-size" initial={4096} min={256} max={131072} value={options.contextSize} onValueChange={set("contextSize")} />
        <OptionRow label="Temperature" flag="--temp" initial={0.8} min={0} max={2} value={options.temperature} onValueChange={set("temperature")} />
        <OptionRow label="Top-P" flag="--top-p" initial={0.95} min={0} max={1} value={options.topP} onValueChange={set("topP")} />
        <OptionRow label="최대 생성 토큰" flag="--n-predict" initial={256} min={1} max={8192} value={options.maxNewTokens} onValueChange={set("maxNewTokens")} />
        <OptionRow label="Seed" flag="--seed" initial={-1} min={-1} max={4294967295} value={options.seed} onValueChange={set("seed")} />
        <OptionRow label="배치 크기" flag="--batch-size" initial={512} min={1} max={2048} value={options.batchSize} onValueChange={set("batchSize")} />
        <OptionRow label="스레드 수" flag="--threads" initial={8} min={1} max={256} value={options.threads} onValueChange={set("threads")} />
      </section>
    </div>
    <div className="panel-footer"><button className="button-primary" type="button" disabled={!pendingRuntime} onClick={runtimeChanged ? onApplyRuntime : onReload}>적용하고 모델 다시 로드</button><p>변경한 런타임과 옵션은 모델을 다시 로드해야 적용됩니다.</p></div>
  </aside>;
}
