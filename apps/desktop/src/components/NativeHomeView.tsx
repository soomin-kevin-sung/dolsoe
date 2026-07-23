import {
  Activity,
  ArrowUp,
  Box,
  Check,
  ChevronRight,
  Code2,
  Cpu,
  Download,
  FileText,
  FolderOpen,
  ListChecks,
  LockKeyhole,
  LoaderCircle,
  MessageSquare,
  RotateCw,
  Sparkles,
  TriangleAlert,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { readinessStatus, type HomeReadinessKind } from "../services/homeReadiness";
import { formatBytes, type AvailableRuntimePack, type RuntimeInstallState } from "../services/runtimePacks";
import type { Session } from "../services/runtime";

interface Props {
  readiness: HomeReadinessKind;
  modelName: string;
  backend: string;
  sessions: Session[];
  cpuPack?: AvailableRuntimePack;
  installState: RuntimeInstallState | null;
  modelProgress: number | null;
  onInstallCpu(): void;
  onRefreshCatalog(): void;
  onCancelInstall(): void;
  onRestart(): void;
  onDismissInstall(): void;
  onChooseModel(): void;
  onCancelModelLoad(): void;
  onStartPrompt(prompt: string): boolean | void | Promise<boolean | void>;
  onOpenDiagnostics(): void;
  onSelectSession(id: string): void;
}

const failureCopy: Record<string, [string, string]> = {
  "runtime-failed-network": ["인터넷 연결을 확인하세요", "CPU 런타임 정보를 받아오지 못했습니다."],
  "runtime-failed-verification": ["다운로드 파일을 확인하지 못했습니다", "파일 체크섬이 일치하지 않습니다."],
  "runtime-failed-disk": ["저장 공간이 부족합니다", "여유 공간을 확보한 뒤 다시 시도하세요."],
  "runtime-failed-recovery": ["CPU 런타임을 불러오지 못했습니다", "검증된 CPU 런타임을 다시 설치하세요."],
  "runtime-failed-unknown": ["CPU 런타임을 설치하지 못했습니다", "다시 시도하거나 진단에서 자세한 내용을 확인하세요."],
};

function SetupBand({ icon, title, body, meta, tone = "neutral", children }: {
  icon: ReactNode;
  title: string;
  body: string;
  meta?: ReactNode;
  tone?: "neutral" | "loading" | "pending" | "error";
  children?: ReactNode;
}) {
  return <section className={`home-setup-band ${tone}`}>
    <span className="home-setup-icon" aria-hidden="true">{icon}</span>
    <div className="home-setup-copy"><h2>{title}</h2><p>{body}</p>{meta && <div className="home-setup-meta">{meta}</div>}</div>
    {children && <div className="home-setup-actions">{children}</div>}
  </section>;
}

export function NativeHomeView(props: Props) {
  const [confirmInstall, setConfirmInstall] = useState(false);
  const [draft, setDraft] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const primaryRef = useRef<HTMLButtonElement>(null);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const confirmButtonRef = useRef<HTMLButtonElement>(null);
  const status = readinessStatus(props.readiness);
  const ready = props.readiness === "ready";
  const progress = props.readiness === "model-loading"
    ? Math.round((props.modelProgress ?? 0) * 100)
    : props.installState?.progress ?? 0;
  const announcement = useMemo(() => props.readiness === "runtime-downloading"
    ? `CPU 런타임 다운로드 ${Math.floor(progress / 10) * 10}%`
    : status.text, [progress, props.readiness, status.text]);

  useEffect(() => {
    const id = requestAnimationFrame(() => ready ? promptRef.current?.focus() : primaryRef.current?.focus());
    return () => cancelAnimationFrame(id);
  }, [props.readiness, ready]);

  useEffect(() => {
    if (!confirmInstall) return;
    const id = requestAnimationFrame(() => confirmButtonRef.current?.focus());
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      event.preventDefault();
      setConfirmInstall(false);
      requestAnimationFrame(() => primaryRef.current?.focus());
    }
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      cancelAnimationFrame(id);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [confirmInstall]);

  function retryRuntime() {
    if (props.readiness === "runtime-failed-network") props.onRefreshCatalog();
    else props.onInstallCpu();
  }

  async function submitPrompt() {
    const prompt = draft.trim();
    if (!ready || !prompt || submitting) return;
    setSubmitting(true);
    try {
      const sent = await props.onStartPrompt(prompt);
      if (sent !== false) setDraft("");
    } finally {
      setSubmitting(false);
    }
  }

  function readinessBand() {
    if (props.readiness === "runtime-missing" || props.readiness === "runtime-checking") {
      const checking = props.readiness === "runtime-checking";
      return <SetupBand
        icon={checking ? <LoaderCircle className="spin" /> : <Cpu />}
        title={checking ? "런타임 정보를 확인하고 있습니다" : "로컬 추론 엔진을 준비합니다"}
        body={checking ? "설치 가능한 CPU 런타임을 불러오는 중입니다." : "CPU 런타임을 한 번 설치하면 이후에는 바로 사용할 수 있습니다."}
        meta={!checking && props.cpuPack ? <><span>{formatBytes(props.cpuPack.sizeBytes)}</span><span>v{props.cpuPack.releaseVersion}</span><span>SHA-256 검증</span></> : undefined}
        tone={checking ? "loading" : "neutral"}
      >
        <button ref={primaryRef} className="button-primary" type="button" disabled={checking || !props.cpuPack} onClick={() => setConfirmInstall(true)}><Download size={14} />설치하기</button>
      </SetupBand>;
    }

    if (["runtime-downloading", "runtime-verifying", "runtime-installing"].includes(props.readiness)) {
      const label = props.readiness === "runtime-downloading" ? "다운로드 중" : props.readiness === "runtime-verifying" ? "검증 중" : "설치 중";
      const bytes = props.readiness === "runtime-downloading" && props.installState
        ? `${formatBytes(props.installState.downloadedBytes)} / ${formatBytes(props.installState.totalBytes)}` : null;
      return <SetupBand icon={<LoaderCircle className="spin" />} title={`CPU 런타임 ${label}`} body="앱을 닫지 않아도 곧 다음 단계로 이어집니다." tone="loading"
        meta={<div className="home-inline-progress"><div className="progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progress} aria-label={label}><div className="progress-fill" style={{ width: `${progress}%` }} /></div><span>{bytes ? `${bytes} · ` : ""}{progress}%</span></div>}>
        {props.readiness === "runtime-downloading" && <button className="button-secondary" type="button" onClick={props.onCancelInstall}>취소</button>}
      </SetupBand>;
    }

    if (props.readiness === "runtime-installed") return <SetupBand icon={<RotateCw />} title="CPU 런타임 교체가 준비되었습니다" body="현재 사용 중인 DLL을 안전하게 교체하려면 앱을 한 번 재시작해야 합니다." tone="pending">
      <button ref={primaryRef} className="button-primary" type="button" onClick={props.onRestart}>지금 재시작</button>
      <button className="button-secondary" type="button" onClick={props.onDismissInstall}>나중에</button>
    </SetupBand>;

    if (props.readiness.startsWith("runtime-failed-")) {
      const [title, body] = failureCopy[props.readiness] ?? failureCopy["runtime-failed-unknown"];
      const network = props.readiness === "runtime-failed-network";
      return <SetupBand icon={network ? <Cpu /> : <TriangleAlert />} title={title} body={body} tone={network ? "neutral" : "error"}>
        <button ref={primaryRef} className="button-primary" type="button" onClick={retryRuntime}><RotateCw size={14} />다시 시도</button>
        <button className="button-secondary" type="button" onClick={props.onOpenDiagnostics}><Activity size={14} />진단</button>
      </SetupBand>;
    }

    if (props.readiness === "model-missing") return <SetupBand icon={<Box />} title="사용할 GGUF 모델을 선택하세요" body="모델 파일을 선택하면 이 자리에서 바로 대화를 시작할 수 있습니다." meta={<><span>.gguf</span><span>로컬 전용</span></>}>
      <button ref={primaryRef} className="button-primary" type="button" onClick={props.onChooseModel}><FolderOpen size={14} />모델 선택</button>
    </SetupBand>;

    if (props.readiness === "model-loading") return <SetupBand icon={<LoaderCircle className="spin" />} title="모델을 메모리에 올리고 있습니다" body={`${props.modelName} · ${props.backend}`} tone="loading"
      meta={<div className="home-inline-progress"><div className="progress-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={progress} aria-label="모델 로딩 중"><div className="progress-fill" style={{ width: `${progress}%` }} /></div><span>{progress}%</span></div>}>
      <button className="button-secondary" type="button" onClick={props.onCancelModelLoad}>취소</button>
    </SetupBand>;

    return null;
  }

  const recent = props.sessions.slice(0, 3);
  const statusDetail = ready ? `${props.modelName} · ${props.backend}` : status.text;
  const cpuReady = ["model-missing", "model-loading", "ready"].includes(props.readiness);
  const setupStep = cpuReady ? 2 : 1;

  return <>
    <div className={`home-view home-focus-view ${ready ? "home-ready" : "home-setup"}`}>
      {ready ? <>
        <section className="home-focus-main">
          <span className="home-welcome-mark" aria-hidden="true"><Sparkles size={18} /></span>
          <h1>무엇을 함께 정리할까요?</h1>
          <p className="home-privacy-copy"><LockKeyhole size={13} />대화와 문서는 이 기기 안에만 저장됩니다.</p>
          <form className="home-prompt-composer" aria-label="새 대화 시작" onSubmit={(event) => { event.preventDefault(); void submitPrompt(); }}>
            <textarea ref={promptRef} aria-label="첫 메시지" rows={3} placeholder="질문하거나, 생각을 정리하거나, 문서를 탐색해 보세요" value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void submitPrompt(); } }} />
            <div className="home-prompt-footer">
              <span className="home-model-label"><span className="status-dot ready" />{statusDetail}</span>
              <button className="home-prompt-send" type="submit" disabled={!draft.trim() || submitting} aria-label="대화 시작"><ArrowUp size={16} /></button>
            </div>
          </form>
          <div className="home-quick-actions" aria-label="추천 작업">
            <button type="button" onClick={() => setDraft("이 문서의 핵심 내용을 요약해줘.")}><FileText size={14} />문서 요약</button>
            <button type="button" onClick={() => setDraft("내 생각을 논리적인 항목으로 정리해줘.")}><ListChecks size={14} />생각 정리</button>
            <button type="button" onClick={() => setDraft("이 코드가 어떻게 동작하는지 설명해줘.")}><Code2 size={14} />코드 설명</button>
          </div>
        </section>

        <section className="home-recent" aria-labelledby="home-recent-title">
          <div className="home-section-heading"><h2 id="home-recent-title">최근 작업</h2></div>
          {recent.length ? <ul>{recent.map((session) => <li key={session.id}><button type="button" className="home-recent-row" onClick={() => props.onSelectSession(session.id)}><span className="home-recent-icon"><MessageSquare size={14} /></span><span className="home-recent-copy"><strong>{session.title}</strong><small>로컬 대화</small></span><time>{session.meta}</time><ChevronRight size={14} aria-hidden="true" /></button></li>)}</ul> : <p className="home-recent-empty">첫 질문을 시작하면 최근 작업이 여기에 표시됩니다.</p>}
        </section>
      </> : <>
        <section className="home-setup-intro">
          <span className="home-welcome-mark" aria-hidden="true"><Cpu size={18} /></span>
          <h1>한 번만 준비하면 됩니다</h1>
          <p>추론에 필요한 CPU 엔진과 사용할 모델을 이 기기에 준비합니다.</p>
        </section>
        <ol className="home-setup-steps" aria-label="로컬 AI 준비 단계">
          <li className={setupStep === 1 ? "active" : "complete"}><span>{cpuReady ? <Check size={13} /> : "1"}</span><div><strong>CPU 런타임</strong><small>{cpuReady ? "설치됨" : status.text}</small></div></li>
          <li className={setupStep === 2 ? "active" : ""}><span>2</span><div><strong>모델 선택</strong><small>{cpuReady ? status.text : "런타임 설치 후 선택"}</small></div></li>
          <li><span>3</span><div><strong>대화 시작</strong><small>모델 준비 후 바로 사용</small></div></li>
        </ol>
        {readinessBand()}
        <p className="home-offline-note"><LockKeyhole size={13} />다운로드 이후에는 네트워크 연결 없이 사용할 수 있습니다.</p>
      </>}
      <p className="sr-only" aria-live="polite" aria-atomic="true">{announcement}</p>
    </div>

    {confirmInstall && props.cpuPack && <div className="dialog-scrim">
      <div role="dialog" aria-modal="true" aria-labelledby="cpu-runtime-install-title" className="confirm-dialog runtime-install-dialog">
        <h2 id="cpu-runtime-install-title">CPU 백엔드를 설치할까요?</h2>
        <p>llama.cpp {props.cpuPack.llamaCppRelease} 기준의 {formatBytes(props.cpuPack.sizeBytes)} 런타임 팩을 다운로드하고 검증합니다.</p>
        <dl><div><dt>팩 버전</dt><dd>{props.cpuPack.releaseVersion}</dd></div><div><dt>llama.cpp</dt><dd>{props.cpuPack.llamaCppCommit.slice(0, 12)}</dd></div><div><dt>적용</dt><dd>설치 후 바로 사용</dd></div></dl>
        <div className="dialog-actions"><button className="button-secondary" type="button" onClick={() => { setConfirmInstall(false); requestAnimationFrame(() => primaryRef.current?.focus()); }}>취소</button><button ref={confirmButtonRef} className="button-primary" type="button" onClick={() => { props.onInstallCpu(); setConfirmInstall(false); }}>다운로드 및 설치</button></div>
      </div>
    </div>}
  </>;
}
