import { Cpu, X } from "lucide-react";
import { useState } from "react";
import type { RuntimePack, ThemePreference } from "../services/runtime";
import { IconButton } from "./IconButton";
import { OptionRow } from "./OptionRow";
import { PackRow } from "./PackRow";
import { SegmentedControl } from "./SegmentedControl";

export function SettingsPanel({ open, packs, onClose, onReload }: { open: boolean; packs: RuntimePack[]; onClose(): void; onReload(): void }) {
  const [runtime, setRuntime] = useState("cuda");
  const [theme, setTheme] = useState<ThemePreference>((document.documentElement.dataset.theme as ThemePreference) || "system");
  function changeTheme(value: ThemePreference) {
    setTheme(value);
    document.documentElement.dataset.theme = value === "system" ? (matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light") : value;
  }
  return <aside className={`settings-panel ${open ? "open" : ""}`} aria-label="설정" hidden={!open}><div className="panel-header"><h2>설정</h2><IconButton icon={X} label="설정 닫기" onClick={onClose} /></div><div className="panel-body"><section className="settings-section"><h3>모델</h3><strong className="model-file">Qwen2.5-7B-Instruct-Q4_K_M.gguf</strong><p>llama · Q4_K_M · 4.36 GB · 최대 컨텍스트 32,768</p><div className="panel-actions"><button className="button-secondary" type="button">다른 모델 선택…</button><button className="button-secondary" type="button">모델 언로드</button></div></section><section className="settings-section"><h3>런타임 <span className="reload-badge">재로드</span></h3><SegmentedControl label="런타임" value={runtime} onChange={setRuntime} items={[{ value: "cpu", label: "CPU" }, { value: "cuda", label: "CUDA" }, { value: "vulkan", label: "Vulkan" }]} /><p className="device-line"><Cpu size={14} /> NVIDIA GeForce RTX 4070 · 12 GB</p><p>런타임을 변경하면 현재 모델을 다시 로드합니다.</p>{packs.filter((pack) => pack.status !== "installed").map((pack) => <PackRow key={pack.id} pack={pack} />)}</section><section className="settings-section"><h3>추론 옵션</h3><OptionRow label="컨텍스트 길이" flag="--ctx-size" initial={8192} min={512} max={32768} /><OptionRow label="GPU 레이어" flag="--n-gpu-layers" initial={32} min={0} max={32} /><OptionRow label="Temperature" flag="--temp" initial={0.7} min={0} max={2} /><OptionRow label="Top-P" flag="--top-p" initial={0.9} min={0} max={1} /><OptionRow label="최대 생성 토큰" flag="--n-predict" initial={1024} min={1} max={8192} /><OptionRow label="Seed" flag="--seed" initial={-1} min={-1} max={2147483647} /></section><section className="settings-section"><h3>고급 설정</h3><input className="panel-search" aria-label="옵션 검색" placeholder="옵션 검색" /><OptionRow label="배치 크기" flag="--batch-size" initial={512} min={1} max={2048} /><OptionRow label="스레드 수" flag="--threads" initial={8} min={1} max={256} /><OptionRow label="반복 페널티" flag="--repeat-penalty" initial={1.1} min={0} max={2} /><p>런타임이 제공하는 옵션 스키마를 기준으로 표시됩니다. 지원하지 않는 값은 저장 시 오류로 표시됩니다.</p></section><section className="settings-section"><h3>화면</h3><SegmentedControl label="테마" value={theme} onChange={changeTheme} items={[{ value: "light", label: "라이트" }, { value: "dark", label: "다크" }, { value: "system", label: "시스템" }]} /></section></div><div className="panel-footer"><button className="button-primary" type="button" onClick={onReload}>적용하고 모델 다시 로드</button><p>변경한 옵션은 모델을 다시 로드해야 적용됩니다.</p></div></aside>;
}
