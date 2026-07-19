import { useState } from "react";
import { X } from "lucide-react";

import type { NativeOptions } from "../hooks/useNativeRuntime";
import type { AvailableRuntimePack, RuntimeBackend, RuntimeInstallState, RuntimePack, RuntimeSelection } from "../services/runtimePacks";
import { IconButton } from "./IconButton";
import { OptionRow } from "./OptionRow";
import { RuntimeBackendRow } from "./RuntimeBackendRow";

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
  onRestart(): void;
  onDismissInstall(): void;
}

const backends: RuntimeBackend[] = ["cpu", "cuda", "vulkan"];
const labels: Record<RuntimeBackend, string> = { cpu: "CPU", cuda: "CUDA", vulkan: "Vulkan" };

function formatBytes(value: number) {
  return value >= 1024 ** 3 ? `${(value / 1024 ** 3).toFixed(1)} GB` : `${Math.max(1, Math.round(value / 1024 ** 2))} MB`;
}

export function NativeSettingsPanel(props: Props) {
  const { open, modelName, options, runtimePacks, runtimePackError, availableRuntimePacks, installState, distributionError, appliedRuntime, pendingRuntime } = props;
  const [confirmBackend, setConfirmBackend] = useState<RuntimeBackend | null>(null);
  const set = (key: keyof NativeOptions) => (value: number) => props.onOptionsChange({ ...options, [key]: value });
  const runtimeChanged = Boolean(appliedRuntime && pendingRuntime && appliedRuntime.backend !== pendingRuntime.backend);
  const confirmation = confirmBackend ? availableRuntimePacks.find((pack) => pack.backend === confirmBackend) : undefined;
  const installedBackend = installState?.phase === "installed" ? installState.packId as RuntimeBackend : null;

  function requestInstall(backend: RuntimeBackend) {
    if (availableRuntimePacks.some((pack) => pack.backend === backend)) setConfirmBackend(backend);
  }

  return <>
    <aside className={`settings-panel ${open ? "open" : ""}`} aria-label="설정" hidden={!open}>
      <div className="panel-header"><h2>설정</h2><IconButton icon={X} label="설정 닫기" onClick={props.onClose} /></div>
      <div className="panel-body">
        <section className="settings-section">
          <h3>모델</h3>
          <strong className="model-file">{modelName}</strong>
          <p>로컬 GGUF 모델과 메모리 설정입니다.</p>
          <div className="panel-actions"><button className="button-secondary" type="button" onClick={props.onChooseModel}>다른 모델 선택</button><button className="button-secondary" type="button" onClick={props.onUnload}>모델 언로드</button></div>
        </section>

        <section className="settings-section">
          <h3>백엔드 {runtimeChanged && <span className="reload-badge">적용 대기</span>}</h3>
          <div className="runtime-backend-list">
            {backends.map((backend) => {
              const pack = runtimePacks.find((candidate) => candidate.id === backend) ?? {
                id: backend, backend, status: "not-installed" as const, runtimeVersion: null,
                llamaCppCommit: null, abiMajor: null, abiMinor: null, devices: [], error: null,
              };
              return <RuntimeBackendRow key={backend} backend={backend} pack={pack}
                available={availableRuntimePacks.find((candidate) => candidate.backend === backend)}
                active={appliedRuntime?.backend === backend}
                selected={(pendingRuntime ?? appliedRuntime)?.backend === backend}
                installState={installState}
                onSelect={() => props.onRuntimeChange(backend)}
                onInstall={() => requestInstall(backend)}
                onCancel={props.onCancelInstall} />;
            })}
          </div>
          {runtimePackError && <p className="runtime-pack-error">설치된 백엔드를 확인하지 못했습니다. {runtimePackError}</p>}
          {distributionError && <p className="runtime-pack-error">다운로드 정보를 불러오지 못했습니다. CPU는 계속 사용할 수 있습니다. {distributionError}</p>}
          {installedBackend && <div className="runtime-restart-notice" role="status">
            <strong>{labels[installedBackend]} 설치 완료</strong>
            <p>새 DLL을 안전하게 적용하려면 앱을 재시작해야 합니다.</p>
            <div className="panel-actions"><button className="button-primary" type="button" onClick={props.onRestart}>지금 재시작</button><button className="button-secondary" type="button" onClick={props.onDismissInstall}>나중에</button></div>
          </div>}
        </section>

        <section className="settings-section"><h3>추론 옵션</h3>
          <OptionRow label="컨텍스트 길이" flag="--ctx-size" initial={4096} min={256} max={131072} value={options.contextSize} onValueChange={set("contextSize")} />
          <OptionRow label="Temperature" flag="--temp" initial={0.8} min={0} max={2} value={options.temperature} onValueChange={set("temperature")} />
          <OptionRow label="Top-P" flag="--top-p" initial={0.95} min={0} max={1} value={options.topP} onValueChange={set("topP")} />
          <OptionRow label="최대 생성 토큰" flag="--n-predict" initial={256} min={1} max={8192} value={options.maxNewTokens} onValueChange={set("maxNewTokens")} />
          <OptionRow label="Seed" flag="--seed" initial={-1} min={-1} max={4294967295} value={options.seed} onValueChange={set("seed")} />
          <OptionRow label="배치 크기" flag="--batch-size" initial={512} min={1} max={2048} value={options.batchSize} onValueChange={set("batchSize")} />
          <OptionRow label="스레드" flag="--threads" initial={8} min={1} max={256} value={options.threads} onValueChange={set("threads")} />
        </section>
      </div>
      <div className="panel-footer"><button className="button-primary" type="button" disabled={!pendingRuntime} onClick={runtimeChanged ? props.onApplyRuntime : props.onReload}>적용하고 모델 다시 로드</button><p>이미 설치된 백엔드는 모델을 다시 로드한 뒤 적용됩니다.</p></div>
    </aside>

    {confirmBackend && confirmation && <div className="dialog-scrim">
      <div role="dialog" aria-modal="true" aria-labelledby="runtime-install-title" className="confirm-dialog runtime-install-dialog">
        <h2 id="runtime-install-title">{labels[confirmBackend]} 백엔드를 설치할까요?</h2>
        <p>llama.cpp {confirmation.llamaCppRelease} 기준의 {formatBytes(confirmation.sizeBytes)} 런타임 팩을 다운로드하고 검증합니다.</p>
        <dl><div><dt>팩 버전</dt><dd>{confirmation.releaseVersion}</dd></div><div><dt>llama.cpp</dt><dd>{confirmation.llamaCppCommit.slice(0, 12)}</dd></div><div><dt>적용</dt><dd>설치 후 앱 재시작</dd></div></dl>
        <div className="dialog-actions"><button className="button-secondary" type="button" onClick={() => setConfirmBackend(null)}>취소</button><button className="button-primary" type="button" onClick={() => { props.onInstall(confirmBackend); setConfirmBackend(null); }}>다운로드 및 설치</button></div>
      </div>
    </div>}
  </>;
}
