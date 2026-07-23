import { useEffect, useMemo, useState, type ReactNode } from "react";

import { describeOption } from "../services/optionPresentation";

export interface InferenceSettingsValues {
  contextSize: number;
  batchSize: number;
  physicalBatchSize: number;
  threads: number;
  useMmap: boolean;
  maxNewTokens: number;
  temperature: number;
  topK: number;
  topP: number;
  minP: number;
  repeatLastN: number;
  repeatPenalty: number;
  frequencyPenalty: number;
  presencePenalty: number;
  seed: number;
}

type NumericKey = Exclude<keyof InferenceSettingsValues, "useMmap">;

interface ControlsProps {
  values: InferenceSettingsValues;
  activeValues?: InferenceSettingsValues;
  onNumberChange(key: NumericKey, value: number): void;
  onMmapChange?(value: boolean): void;
}

interface OptionFrameProps {
  label: string;
  flag: string;
  description: string;
  metadata: string;
  effect: string;
  children: ReactNode;
}

const tokenPresets = [128, 256, 512, 1024, 2048, 4096, 8192] as const;
const topKPresets = [
  { value: 0, label: "사용 안 함 (0)" },
  { value: 20, label: "좁게 (20)" },
  { value: 40, label: "기본 (40)" },
  { value: 80, label: "넓게 (80)" },
  { value: 100, label: "매우 넓게 (100)" },
] as const;
const repeatWindowPresets = [
  { value: -1, label: "전체 컨텍스트 (-1)" },
  { value: 0, label: "사용 안 함 (0)" },
  { value: 32, label: "최근 32 토큰" },
  { value: 64, label: "기본 · 최근 64 토큰" },
  { value: 128, label: "최근 128 토큰" },
  { value: 256, label: "최근 256 토큰" },
] as const;
const contextPresets = [2048, 4096, 8192, 16384, 32768, 65536, 131072] as const;
const batchPresets = [128, 256, 512, 1024, 2048] as const;
const physicalBatchPresets = [64, 128, 256, 512, 1024, 2048] as const;

function OptionFrame({ label, flag, description, metadata, effect, children }: OptionFrameProps) {
  return <div className="inference-option">
    <div className="inference-option-copy">
      <div className="inference-option-heading"><strong>{label}</strong><code>{flag}</code></div>
      <span className="inference-option-description">{description}</span>
      <span className="inference-option-metadata">{metadata}</span>
    </div>
    <div className="inference-option-control">
      {children}
      <span className="inference-option-effect">{effect}</span>
    </div>
  </div>;
}

function nearestPresetIndex(values: readonly number[], value: number) {
  return values.reduce((best, candidate, index) => (
    Math.abs(candidate - value) < Math.abs(values[best] - value) ? index : best
  ), 0);
}

function formatTokenMark(value: number) {
  return value >= 1024 ? `${value / 1024}K` : String(value);
}

function appliedChange(current: number, next: number, unit = "") {
  if (current === next) return null;
  const suffix = unit ? ` ${unit}` : "";
  return `현재 ${current.toLocaleString("ko-KR")}${suffix} → 변경 ${next.toLocaleString("ko-KR")}${suffix}`;
}

function DiscreteSlider({ label, values, value, onChange }: {
  label: string;
  values: readonly number[];
  value: number;
  onChange(value: number): void;
}) {
  const index = nearestPresetIndex(values, value);
  return <>
    <div className="slider-value">{value.toLocaleString("ko-KR")}</div>
    <input
      className="option-slider discrete"
      type="range"
      min={0}
      max={values.length - 1}
      step={1}
      value={index}
      aria-label={label}
      aria-valuetext={`${value.toLocaleString("ko-KR")} 토큰`}
      onChange={(event) => onChange(values[Number(event.target.value)])}
    />
    <div className="slider-marks" aria-hidden="true">
      {values.map((preset) => <span key={preset}>{formatTokenMark(preset)}</span>)}
    </div>
  </>;
}

function NumericField({ label, value, min, max, step = 1, integer = false, disabled = false, onChange }: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  integer?: boolean;
  disabled?: boolean;
  onChange(value: number): void;
}) {
  const [draft, setDraft] = useState(String(value));

  useEffect(() => {
    setDraft(String(value));
  }, [value]);

  function commit() {
    const parsed = Number(draft);
    const precision = String(step).split(".")[1]?.length ?? 0;
    const stepped = integer ? Math.round(parsed) : Number((Math.round(parsed / step) * step).toFixed(precision));
    const normalized = Number.isFinite(parsed)
      ? Math.min(max, Math.max(min, stepped))
      : value;
    setDraft(String(normalized));
    if (normalized !== value) onChange(normalized);
  }

  return <input
    className="option-number"
    type="text"
    inputMode={integer ? "numeric" : "decimal"}
    aria-label={label}
    value={draft}
    disabled={disabled}
    onChange={(event) => {
      const next = event.target.value;
      const valid = integer ? /^\d*$/.test(next) : /^-?\d*(?:\.\d*)?$/.test(next);
      if (valid) setDraft(next);
    }}
    onKeyDown={(event) => {
      if (event.key === "Enter") event.currentTarget.blur();
    }}
    onBlur={commit}
  />;
}

function RangeWithNumber({ label, value, min, max, step, onChange }: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange(value: number): void;
}) {
  return <div className="range-number-control">
    <input
      className="option-slider"
      type="range"
      min={min}
      max={max}
      step={step}
      value={value}
      aria-label={`${label} 슬라이더`}
      onChange={(event) => onChange(Number(event.target.value))}
    />
    <NumericField label={`${label} 정확한 값`} value={value} min={min} max={max} step={step} onChange={onChange} />
  </div>;
}

function PresetSelect({ label, value, presets, min, max, onChange }: {
  label: string;
  value: number;
  presets: readonly { value: number; label: string }[];
  min: number;
  max: number;
  onChange(value: number): void;
}) {
  const isPreset = presets.some((preset) => preset.value === value);
  const [custom, setCustom] = useState(!isPreset);

  useEffect(() => {
    if (!presets.some((preset) => preset.value === value)) setCustom(true);
  }, [presets, value]);

  return <div className={`preset-control ${custom ? "custom" : ""}`}>
    <select
      className="option-select"
      aria-label={label}
      value={custom ? "custom" : String(value)}
      onChange={(event) => {
        if (event.target.value === "custom") {
          setCustom(true);
          return;
        }
        setCustom(false);
        onChange(Number(event.target.value));
      }}
    >
      {presets.map((preset) => <option key={preset.value} value={preset.value}>{preset.label}</option>)}
      <option value="custom">직접 입력</option>
    </select>
    {custom && <NumericField label={`${label} 직접 입력`} value={value} min={min} max={max} integer onChange={onChange} />}
  </div>;
}

function SeedControl({ value, onChange }: { value: number; onChange(value: number): void }) {
  const random = value === -1;
  const [fixedSeed, setFixedSeed] = useState(value >= 0 ? value : 42);

  useEffect(() => {
    if (value >= 0) setFixedSeed(value);
  }, [value]);

  return <div className="seed-control">
    <label className="check-control">
      <input
        type="checkbox"
        checked={random}
        onChange={(event) => onChange(event.target.checked ? -1 : fixedSeed)}
      />
      <span>매번 무작위</span>
    </label>
    <NumericField
      label="고정 Seed"
      value={fixedSeed}
      min={0}
      max={4294967295}
      integer
      disabled={random}
      onChange={(next) => {
        setFixedSeed(next);
        onChange(next);
      }}
    />
  </div>;
}

function ExactNumberOption({ label, flag, metadata, value, min, max, step, onChange }: {
  label: string;
  flag: string;
  metadata: string;
  value: number;
  min: number;
  max: number;
  step: number;
  onChange(value: number): void;
}) {
  const presentation = describeOption(flag, value);
  return <OptionFrame label={label} flag={flag} description={presentation.description} metadata={metadata} effect={presentation.effect}>
    <NumericField label={label} value={value} min={min} max={max} step={step} onChange={onChange} />
  </OptionFrame>;
}

export function GenerationSettingsControls({ values, onNumberChange }: ControlsProps) {
  const maxTokenEffect = describeOption("--n-predict", values.maxNewTokens);
  const temperature = describeOption("--temp", values.temperature);
  const topP = describeOption("--top-p", values.topP);
  const seed = describeOption("--seed", values.seed);
  const topK = describeOption("--top-k", values.topK);
  const minP = describeOption("--min-p", values.minP);
  const repeatWindow = describeOption("--repeat-last-n", values.repeatLastN);
  const frequency = describeOption("--frequency-penalty", values.frequencyPenalty);
  const presence = describeOption("--presence-penalty", values.presencePenalty);

  return <>
    <OptionFrame label="최대 생성 토큰" flag="--n-predict" description={maxTokenEffect.description} metadata="선택값 128–8,192 · 기본값 256 · 컨텍스트 한도에서 조기 종료" effect={maxTokenEffect.effect}>
      <DiscreteSlider label="최대 생성 토큰" values={tokenPresets} value={values.maxNewTokens} onChange={(value) => onNumberChange("maxNewTokens", value)} />
    </OptionFrame>
    <OptionFrame label="Temperature" flag="--temp" description={temperature.description} metadata="범위 0–2 · 기본값 0.8" effect={temperature.effect}>
      <RangeWithNumber label="Temperature" value={values.temperature} min={0} max={2} step={0.05} onChange={(value) => onNumberChange("temperature", value)} />
    </OptionFrame>
    <OptionFrame label="Top-P" flag="--top-p" description={topP.description} metadata="범위 0–1 · 기본값 0.95 · 1은 제한 없음" effect={topP.effect}>
      <RangeWithNumber label="Top-P" value={values.topP} min={0} max={1} step={0.01} onChange={(value) => onNumberChange("topP", value)} />
    </OptionFrame>
    <OptionFrame label="Seed" flag="--seed" description={seed.description} metadata="고정값 0–4,294,967,295 · 기본값 무작위" effect={seed.effect}>
      <SeedControl value={values.seed} onChange={(value) => onNumberChange("seed", value)} />
    </OptionFrame>

    <div className="inference-option-group-label">고급 샘플링</div>
    <OptionFrame label="Top-K" flag="--top-k" description={topK.description} metadata="앱 허용 범위 0–100,000 · 기본값 40 · 0은 제한 없음" effect={topK.effect}>
      <PresetSelect label="Top-K" value={values.topK} presets={topKPresets} min={0} max={100000} onChange={(value) => onNumberChange("topK", value)} />
    </OptionFrame>
    <OptionFrame label="Min-P" flag="--min-p" description={minP.description} metadata="범위 0–1 · 기본값 0.05 · 0은 사용 안 함" effect={minP.effect}>
      <RangeWithNumber label="Min-P" value={values.minP} min={0} max={1} step={0.01} onChange={(value) => onNumberChange("minP", value)} />
    </OptionFrame>
    <OptionFrame label="반복 검사 범위" flag="--repeat-last-n" description={repeatWindow.description} metadata="-1은 전체 · 0은 사용 안 함 · 기본값 64" effect={repeatWindow.effect}>
      <PresetSelect label="반복 검사 범위" value={values.repeatLastN} presets={repeatWindowPresets} min={-1} max={131072} onChange={(value) => onNumberChange("repeatLastN", value)} />
    </OptionFrame>
    <ExactNumberOption label="반복 페널티" flag="--repeat-penalty" metadata="범위 0–10 · 기본값 1.1 · 1은 사용 안 함" value={values.repeatPenalty} min={0} max={10} step={0.05} onChange={(value) => onNumberChange("repeatPenalty", value)} />
    <OptionFrame label="빈도 페널티" flag="--frequency-penalty" description={frequency.description} metadata="범위 -2–2 · 기본값 0 · 0은 사용 안 함" effect={frequency.effect}>
      <RangeWithNumber label="빈도 페널티" value={values.frequencyPenalty} min={-2} max={2} step={0.05} onChange={(value) => onNumberChange("frequencyPenalty", value)} />
    </OptionFrame>
    <OptionFrame label="존재 페널티" flag="--presence-penalty" description={presence.description} metadata="범위 -2–2 · 기본값 0 · 0은 사용 안 함" effect={presence.effect}>
      <RangeWithNumber label="존재 페널티" value={values.presencePenalty} min={-2} max={2} step={0.05} onChange={(value) => onNumberChange("presencePenalty", value)} />
    </OptionFrame>
  </>;
}

export function PerformanceSettingsControls({ values, activeValues = values, onNumberChange, onMmapChange }: ControlsProps) {
  const autoThreadCount = useMemo(() => Math.min(8, Math.max(1, navigator.hardwareConcurrency || 4)), []);
  const [autoThreads, setAutoThreads] = useState(values.threads === autoThreadCount);
  const context = describeOption("--ctx-size", values.contextSize);
  const batch = describeOption("--batch-size", values.batchSize);
  const physicalBatch = describeOption("--ubatch-size", values.physicalBatchSize);
  const threads = describeOption("--threads", values.threads);
  const contextEffect = appliedChange(activeValues.contextSize, values.contextSize, "토큰") ?? context.effect;
  const batchEffect = appliedChange(activeValues.batchSize, values.batchSize, "토큰") ?? batch.effect;
  const physicalBatchEffect = appliedChange(activeValues.physicalBatchSize, values.physicalBatchSize, "토큰") ?? physicalBatch.effect;
  const threadEffect = appliedChange(activeValues.threads, values.threads, "개")
    ?? (autoThreads ? `이 장치에서 ${autoThreadCount}개 자동 선택` : threads.effect);
  const mmapEffect = activeValues.useMmap === values.useMmap
    ? values.useMmap ? "메모리 매핑 사용" : "모델 파일을 직접 읽음"
    : `현재 ${activeValues.useMmap ? "사용" : "사용 안 함"} → 변경 ${values.useMmap ? "사용" : "사용 안 함"}`;

  return <>
    <OptionFrame label="컨텍스트 길이" flag="--ctx-size" description={context.description} metadata="선택값 2,048–131,072 · 기본값 4,096 · 모델 한도 확인 필요" effect={contextEffect}>
      <DiscreteSlider label="컨텍스트 길이" values={contextPresets} value={values.contextSize} onChange={(value) => onNumberChange("contextSize", value)} />
    </OptionFrame>
    <OptionFrame label="배치 크기" flag="--batch-size" description={batch.description} metadata="선택값 128–2,048 · 기본값 512" effect={batchEffect}>
      <DiscreteSlider label="배치 크기" values={batchPresets} value={values.batchSize} onChange={(value) => onNumberChange("batchSize", value)} />
    </OptionFrame>
    <OptionFrame label="물리 배치 크기" flag="--ubatch-size" description={physicalBatch.description} metadata="선택값 64–2,048 · 기본값 128" effect={physicalBatchEffect}>
      <DiscreteSlider label="물리 배치 크기" values={physicalBatchPresets} value={values.physicalBatchSize} onChange={(value) => onNumberChange("physicalBatchSize", value)} />
    </OptionFrame>
    <OptionFrame label="스레드" flag="--threads" description={threads.description} metadata="앱 범위 1–256 · 장치 기준 자동 선택" effect={threadEffect}>
      <div className="seed-control">
        <label className="check-control">
          <input
            type="checkbox"
            checked={autoThreads}
            onChange={(event) => {
              setAutoThreads(event.target.checked);
              if (event.target.checked) onNumberChange("threads", autoThreadCount);
            }}
          />
          <span>자동 선택</span>
        </label>
        <NumericField label="스레드 수" value={values.threads} min={1} max={256} integer disabled={autoThreads} onChange={(value) => onNumberChange("threads", value)} />
      </div>
    </OptionFrame>
    <OptionFrame label="메모리 매핑" flag="--mmap" description="모델 파일을 운영체제의 가상 메모리에 연결합니다." metadata="llama.cpp 기본값: 사용" effect={mmapEffect}>
      <label className="switch-control">
        <input type="checkbox" checked={values.useMmap} aria-label="메모리 매핑" onChange={(event) => onMmapChange?.(event.target.checked)} />
        <span aria-hidden="true" />
        <strong>{values.useMmap ? "사용" : "사용 안 함"}</strong>
      </label>
    </OptionFrame>
  </>;
}
