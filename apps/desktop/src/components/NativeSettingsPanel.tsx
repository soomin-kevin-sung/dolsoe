import { useEffect, useState } from "react";

import type { StartPagePreference } from "../hooks/useGeneralPreferences";
import type { NativeOptions } from "../hooks/useNativeRuntime";
import type { ThemePreference } from "../services/runtime";
import { formatBytes, type AvailableRuntimePack, type RuntimeBackend, type RuntimeInstallState, type RuntimePack, type RuntimeSelection } from "../services/runtimePacks";
import { GenerationSettingsControls, PerformanceSettingsControls } from "./InferenceSettingsControls";
import { AgentModeSettings } from "./AgentModeSettings";
import { GeneralSettingsControls } from "./GeneralSettingsControls";
import { PersonaPromptSettings } from "./PersonaPromptSettings";
import { RuntimeBackendRow } from "./RuntimeBackendRow";
import { SettingsDialog, type SettingsTab } from "./SettingsDialog";
import { StopSequenceOption } from "./StopSequenceOption";

interface Props {
  open: boolean;
  initialTab?: SettingsTab;
  modelLoaded: boolean;
  theme: ThemePreference;
  startPage: StartPagePreference;
  autoLoadLastModel: boolean;
  options: NativeOptions;
  runtimePacks: RuntimePack[];
  runtimePackError: string | null;
  availableRuntimePacks: AvailableRuntimePack[];
  installState: RuntimeInstallState | null;
  distributionError: string | null;
  distributionLoading: boolean;
  appliedRuntime: RuntimeSelection | null;
  reloadDisabled?: boolean;
  onThemeChange(theme: ThemePreference): void;
  onStartPageChange(startPage: StartPagePreference): void;
  onAutoLoadLastModelChange(enabled: boolean): void;
  onOptionsChange(options: NativeOptions): void;
  onApplyConfiguration(options: NativeOptions, backend: RuntimeBackend): Promise<boolean>;
  onClose(): void;
  onInstall(packId: string): void;
  onCancelInstall(): void;
  onRestart(): void;
  onDismissInstall(): void;
}

const backends: RuntimeBackend[] = ["cpu", "cuda", "vulkan"];
const labels: Record<RuntimeBackend, string> = { cpu: "CPU", cuda: "CUDA", vulkan: "Vulkan" };
const performanceKeys = ["contextSize", "batchSize", "physicalBatchSize", "threads", "useMmap"] as const;
const nativeSettingsTabs: SettingsTab[] = ["general", "persona", "agent", "generation", "performance", "runtime"];

function mergePerformanceOptions(active: NativeOptions, draft: NativeOptions): NativeOptions {
  return performanceKeys.reduce((next, key) => ({ ...next, [key]: draft[key] }), active);
}

export function NativeSettingsPanel(props: Props) {
  const { open, options, runtimePacks, runtimePackError, availableRuntimePacks, installState, distributionError, distributionLoading, appliedRuntime } = props;
  const [confirmBackend, setConfirmBackend] = useState<RuntimeBackend | null>(null);
  const [activeTab, setActiveTab] = useState<SettingsTab>(() => props.initialTab ?? (installState?.phase === "downloading" ? "runtime" : "general"));
  const [draftOptions, setDraftOptions] = useState(options);
  const [draftBackend, setDraftBackend] = useState<RuntimeBackend>(appliedRuntime?.backend ?? "cpu");
  const [applying, setApplying] = useState(false);
  const [applyError, setApplyError] = useState<string | null>(null);
  const setNumber = (key: keyof NativeOptions) => (value: number) => props.onOptionsChange({ ...options, [key]: value });
  const setDraftNumber = (key: keyof NativeOptions) => (value: number) => setDraftOptions((current) => ({ ...current, [key]: value }));
  const runtimeChanged = Boolean(appliedRuntime && appliedRuntime.backend !== draftBackend);
  const performanceChanged = performanceKeys.some((key) => options[key] !== draftOptions[key]);
  const changedCount = performanceKeys.filter((key) => options[key] !== draftOptions[key]).length + (runtimeChanged ? 1 : 0);
  const hasDraftChanges = performanceChanged || runtimeChanged;
  const confirmation = confirmBackend ? availableRuntimePacks.find((pack) => pack.backend === confirmBackend) : undefined;
  const installedBackend = installState?.phase === "installed" ? installState.packId as RuntimeBackend : null;
  const restartRequired = installedBackend !== null && installState?.restartRequired === true;

  useEffect(() => {
    if (!open) return;
    if (props.initialTab) setActiveTab(props.initialTab);
    setDraftOptions(options);
    setDraftBackend(appliedRuntime?.backend ?? "cpu");
    setApplying(false);
    setApplyError(null);
  }, [open, props.initialTab]);

  function requestInstall(backend: RuntimeBackend) {
    if (availableRuntimePacks.some((pack) => pack.backend === backend)) setConfirmBackend(backend);
  }

  async function applyDraft() {
    setApplying(true);
    setApplyError(null);
    const applied = await props.onApplyConfiguration(mergePerformanceOptions(options, draftOptions), draftBackend);
    setApplying(false);
    if (applied) props.onClose();
    else setApplyError("새 설정을 적용하지 못해 이전 설정으로 복구했습니다.");
  }

  const footer = hasDraftChanges ? <>
    <div className="settings-apply-copy">
      <strong>변경사항 {changedCount}개</strong>
      <span>{props.reloadDisabled ? "현재 생성이 끝난 뒤 다시 로드할 수 있습니다." : "재로드하지 않고 닫으면 변경사항이 취소됩니다."}</span>
      {applyError && <span className="settings-apply-error" role="alert">{applyError}</span>}
    </div>
    <button className="button-primary" type="button" disabled={props.reloadDisabled || applying} onClick={() => void applyDraft()}>
      {applying ? "적용하는 중..." : props.modelLoaded ? "모델 다시 로드" : "변경사항 적용"}
    </button>
  </> : undefined;

  return <>
    <SettingsDialog open={open} activeTab={activeTab} closeOnEscape={!confirmBackend} availableTabs={nativeSettingsTabs} onTabChange={setActiveTab} onClose={props.onClose} footer={footer}>
      {activeTab === "general" && <div id="settings-panel-general" role="tabpanel">
        <GeneralSettingsControls
          theme={props.theme}
          startPage={props.startPage}
          autoLoadLastModel={props.autoLoadLastModel}
          onThemeChange={props.onThemeChange}
          onStartPageChange={props.onStartPageChange}
          onAutoLoadLastModelChange={props.onAutoLoadLastModelChange}
        />
      </div>}

      {activeTab === "persona" && <PersonaPromptSettings active={open && activeTab === "persona"} />}

      {activeTab === "agent" && <AgentModeSettings />}

      {activeTab === "runtime" && <div id="settings-panel-runtime" role="tabpanel">
        <section className="settings-section settings-section-first">
          <h3>백엔드 {runtimeChanged && <span className="reload-badge">변경됨</span>}</h3>
          <div className="runtime-backend-list">
            {backends.map((backend) => {
              const pack = runtimePacks.find((candidate) => candidate.id === backend) ?? {
                id: backend, backend, status: "not-installed" as const, runtimeVersion: null,
                llamaCppCommit: null, abiMajor: null, abiMinor: null, devices: [], error: null,
              };
              return <RuntimeBackendRow key={backend} backend={backend} pack={pack}
                available={availableRuntimePacks.find((candidate) => candidate.backend === backend)}
                active={appliedRuntime?.backend === backend}
                selected={draftBackend === backend}
                installState={installState}
                onSelect={() => setDraftBackend(backend)}
                onInstall={() => requestInstall(backend)}
                onCancel={props.onCancelInstall} />;
            })}
          </div>
          {distributionLoading && <p className="runtime-catalog-status" role="status">다운로드 정보 확인 중...</p>}
          {runtimePackError && <p className="runtime-pack-error">설치된 백엔드를 확인하지 못했습니다. {runtimePackError}</p>}
          {distributionError && <p className="runtime-pack-error">다운로드 정보를 불러오지 못했습니다. 이미 설치된 백엔드는 계속 사용할 수 있습니다. {distributionError}</p>}
          {installedBackend && <div className={`runtime-install-result ${restartRequired ? "pending" : "success"}`} role="status">
            <strong>{labels[installedBackend]} 설치 완료</strong>
            <p>{restartRequired ? "현재 사용 중인 DLL을 안전하게 교체하려면 앱을 재시작해야 합니다." : installedBackend === "cpu" ? "런타임을 바로 사용할 수 있습니다." : "백엔드 목록에서 선택한 뒤 모델에 적용할 수 있습니다."}</p>
            <div className="panel-actions">{restartRequired && <button className="button-primary" type="button" onClick={props.onRestart}>지금 재시작</button>}<button className="button-secondary" type="button" onClick={props.onDismissInstall}>{restartRequired ? "나중에" : "확인"}</button></div>
          </div>}
        </section>
      </div>}

      {activeTab === "generation" && <div id="settings-panel-generation" role="tabpanel">
        <section className="settings-section settings-section-first"><h3>생성 옵션 <span className="apply-badge">다음 응답</span></h3>
          <GenerationSettingsControls values={options} onNumberChange={(key, value) => setNumber(key)(value)} />
          <StopSequenceOption values={options.stopSequences} onChange={(stopSequences) => props.onOptionsChange({ ...options, stopSequences })} />
        </section>
      </div>}

      {activeTab === "performance" && <div id="settings-panel-performance" role="tabpanel">
        <section className="settings-section settings-section-first"><h3>성능 <span className="reload-badge">모델 재로드</span></h3>
          <PerformanceSettingsControls
            values={draftOptions}
            activeValues={options}
            onNumberChange={(key, value) => setDraftNumber(key)(value)}
            onMmapChange={(useMmap) => setDraftOptions((current) => ({ ...current, useMmap }))}
          />
        </section>
      </div>}
    </SettingsDialog>

    {confirmBackend && confirmation && <div className="dialog-scrim">
      <div role="dialog" aria-modal="true" aria-labelledby="runtime-install-title" className="confirm-dialog runtime-install-dialog">
        <h2 id="runtime-install-title">{labels[confirmBackend]} 백엔드를 설치할까요?</h2>
        <p>llama.cpp {confirmation.llamaCppRelease} 기준의 {formatBytes(confirmation.sizeBytes)} 런타임 팩을 다운로드하고 검증합니다.</p>
        <dl><div><dt>팩 버전</dt><dd>{confirmation.releaseVersion}</dd></div><div><dt>llama.cpp</dt><dd>{confirmation.llamaCppCommit.slice(0, 12)}</dd></div><div><dt>적용</dt><dd>보통 즉시 · 사용 중 DLL 교체 시 재시작</dd></div></dl>
        <div className="dialog-actions"><button className="button-secondary" type="button" onClick={() => setConfirmBackend(null)}>취소</button><button className="button-primary" type="button" onClick={() => { props.onInstall(confirmBackend); setConfirmBackend(null); }}>다운로드 및 설치</button></div>
      </div>
    </div>}
  </>;
}
