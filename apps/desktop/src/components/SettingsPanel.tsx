import { Cpu } from "lucide-react";
import { useEffect, useState } from "react";

import type { StartPagePreference } from "../hooks/useGeneralPreferences";
import type { RuntimePack, ThemePreference } from "../services/runtime";
import { GeneralSettingsControls } from "./GeneralSettingsControls";
import { GenerationSettingsControls, PerformanceSettingsControls, type InferenceSettingsValues } from "./InferenceSettingsControls";
import { PackRow } from "./PackRow";
import { SegmentedControl } from "./SegmentedControl";
import { SettingsDialog, type SettingsTab } from "./SettingsDialog";
import { StopSequenceOption } from "./StopSequenceOption";

const runtimeItems: { value: RuntimePack["id"]; label: string }[] = [
  { value: "cpu", label: "CPU" },
  { value: "cuda", label: "CUDA" },
  { value: "vulkan", label: "Vulkan" },
];
const performanceKeys = ["contextSize", "batchSize", "physicalBatchSize", "threads", "useMmap"] as const;

interface Props {
  open: boolean;
  initialTab?: SettingsTab;
  packs: RuntimePack[];
  theme: ThemePreference;
  startPage: StartPagePreference;
  autoLoadLastModel: boolean;
  onThemeChange(theme: ThemePreference): void;
  onStartPageChange(startPage: StartPagePreference): void;
  onAutoLoadLastModelChange(enabled: boolean): void;
  onClose(): void;
}

export function SettingsPanel({ open, initialTab, packs, theme, startPage, autoLoadLastModel, onThemeChange, onStartPageChange, onAutoLoadLastModelChange, onClose }: Props) {
  const [runtime, setRuntime] = useState<RuntimePack["id"]>("cpu");
  const [draftRuntime, setDraftRuntime] = useState<RuntimePack["id"]>("cpu");
  const [stopSequences, setStopSequences] = useState<string[]>([]);
  const [options, setOptions] = useState<InferenceSettingsValues>({
    contextSize: 4096,
    batchSize: 512,
    physicalBatchSize: 128,
    threads: Math.min(8, Math.max(1, navigator.hardwareConcurrency || 4)),
    useMmap: true,
    maxNewTokens: 256,
    temperature: 0.8,
    topK: 40,
    topP: 0.95,
    minP: 0.05,
    repeatLastN: 64,
    repeatPenalty: 1.1,
    frequencyPenalty: 0,
    presencePenalty: 0,
    seed: -1,
  });
  const [draftOptions, setDraftOptions] = useState(options);
  const [activeTab, setActiveTab] = useState<SettingsTab>(() => initialTab ?? (packs.some((pack) => pack.status === "installing") ? "runtime" : "general"));
  const selectedPack = packs.find((pack) => pack.id === runtime);
  const runtimeChanged = draftRuntime !== runtime;
  const performanceChanged = performanceKeys.some((key) => options[key] !== draftOptions[key]);
  const changedCount = performanceKeys.filter((key) => options[key] !== draftOptions[key]).length + (runtimeChanged ? 1 : 0);
  const hasDraftChanges = runtimeChanged || performanceChanged;
  const installBusy = packs.some((pack) => pack.status === "installing");

  useEffect(() => {
    if (!open) return;
    if (initialTab) setActiveTab(initialTab);
    setDraftRuntime(runtime);
    setDraftOptions(options);
  }, [initialTab, open]);

  const footer = hasDraftChanges ? <>
    <div className="settings-apply-copy">
      <strong>변경사항 {changedCount}개</strong>
      <span>재로드하지 않고 닫으면 변경사항이 취소됩니다.</span>
    </div>
    <button className="button-primary" type="button" onClick={() => {
      setRuntime(draftRuntime);
      setOptions(draftOptions);
      onClose();
    }}>모델 다시 로드</button>
  </> : undefined;

  return <SettingsDialog open={open} activeTab={activeTab} onTabChange={setActiveTab} onClose={onClose} footer={footer}>
    {activeTab === "general" && <div id="settings-panel-general" role="tabpanel">
      <GeneralSettingsControls
        theme={theme}
        startPage={startPage}
        autoLoadLastModel={autoLoadLastModel}
        onThemeChange={onThemeChange}
        onStartPageChange={onStartPageChange}
        onAutoLoadLastModelChange={onAutoLoadLastModelChange}
      />
    </div>}

    {activeTab === "runtime" && <div id="settings-panel-runtime" role="tabpanel">
      <section className="settings-section settings-section-first">
        <h3>런타임 {runtimeChanged && <span className="reload-badge">변경됨</span>}</h3>
        <SegmentedControl<RuntimePack["id"]> label="런타임" value={draftRuntime} onChange={setDraftRuntime} items={runtimeItems.map((item) => ({ ...item, disabled: packs.find((pack) => pack.id === item.value)?.status !== "installed", title: packs.find((pack) => pack.id === item.value)?.status === "installed" ? undefined : `${item.label} 런타임이 설치되어 있지 않습니다.` }))} />
        <p className="device-line"><Cpu size={14} /> {runtime === "cuda" ? "NVIDIA GeForce RTX 4070" : "이 PC의 CPU"} · {selectedPack?.version}</p>
        {runtimeItems.filter((item) => packs.find((pack) => pack.id === item.value)?.status === "available").map((item) => <p className="runtime-unavailable" key={item.value}>{item.label} 런타임이 설치되어 있지 않습니다.</p>)}
        {packs.filter((pack) => pack.status === "installing" || pack.status === "available").map((pack) => <PackRow key={pack.id} pack={pack} installBusy={installBusy} />)}
      </section>
    </div>}

    {activeTab === "generation" && <div id="settings-panel-generation" role="tabpanel">
      <section className="settings-section settings-section-first">
        <h3>생성 옵션 <span className="apply-badge">다음 응답</span></h3>
        <GenerationSettingsControls values={options} onNumberChange={(key, value) => setOptions((current) => ({ ...current, [key]: value }))} />
        <StopSequenceOption values={stopSequences} onChange={setStopSequences} />
      </section>
    </div>}

    {activeTab === "performance" && <div id="settings-panel-performance" role="tabpanel">
      <section className="settings-section settings-section-first">
        <h3>성능 <span className="reload-badge">모델 재로드</span></h3>
        <PerformanceSettingsControls
          values={draftOptions}
          activeValues={options}
          onNumberChange={(key, value) => setDraftOptions((current) => ({ ...current, [key]: value }))}
          onMmapChange={(useMmap) => setDraftOptions((current) => ({ ...current, useMmap }))}
        />
      </section>
    </div>}
  </SettingsDialog>;
}
