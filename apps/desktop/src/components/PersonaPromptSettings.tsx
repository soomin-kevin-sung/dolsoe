import { openPath } from "@tauri-apps/plugin-opener";
import { Eye, FolderOpen, RefreshCw, RotateCcw, Save } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import {
  PersonaPromptService,
  promptDraftFromState,
  samePromptDraft,
  type PersonaPromptDraft,
  type PersonaPromptState,
} from "../services/personaPrompts";

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

export function PersonaPromptSettings({ active }: { active: boolean }) {
  const service = useMemo(() => new PersonaPromptService(), []);
  const [saved, setSaved] = useState<PersonaPromptState | null>(null);
  const [draft, setDraft] = useState<PersonaPromptDraft | null>(null);
  const [preview, setPreview] = useState<PersonaPromptState | null>(null);
  const [selectedPath, setSelectedPath] = useState("");
  const [loading, setLoading] = useState(false);
  const [loadAttempted, setLoadAttempted] = useState(false);
  const [saving, setSaving] = useState(false);
  const [resetArmed, setResetArmed] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function load() {
    setLoadAttempted(true);
    setLoading(true);
    setError(null);
    setStatus(null);
    try {
      const next = await service.getState();
      setSaved(next);
      setDraft(promptDraftFromState(next));
      setPreview(next);
      setSelectedPath((current) => (
        next.documents.some((document) => document.path === current)
          ? current
          : next.documents[0]?.path ?? ""
      ));
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (active && !saved && !loadAttempted) void load();
  }, [active, loadAttempted, saved]);

  useEffect(() => {
    if (!active || !draft) return;
    let disposed = false;
    const timer = window.setTimeout(() => {
      void service.preview(draft)
        .then((next) => {
          if (!disposed) {
            setPreview(next);
            setError(null);
          }
        })
        .catch((nextError) => {
          if (!disposed) setError(errorText(nextError));
        });
    }, 240);
    return () => {
      disposed = true;
      window.clearTimeout(timer);
    };
  }, [active, draft, service]);

  if (!loading && error && (!saved || !draft)) {
    return <div className="persona-loading persona-load-error" role="alert">
      <span>{error}</span>
      <button className="button-secondary" type="button" onClick={() => void load()}>
        <RefreshCw size={14} aria-hidden="true" />다시 시도
      </button>
    </div>;
  }

  if (loading || !saved || !draft) {
    return <div className="persona-loading" role="status">프롬프트 설정을 불러오는 중...</div>;
  }

  const selected = saved.documents.find((document) => document.path === selectedPath)
    ?? saved.documents[0];
  const selectedDraft = draft.documents.find((document) => document.path === selected?.path);
  const dirty = !samePromptDraft(draft, promptDraftFromState(saved));

  function updateDocument(content: string) {
    if (!selected) return;
    setDraft((current) => current ? {
      ...current,
      documents: current.documents.map((document) => (
        document.path === selected.path ? { ...document, content } : document
      )),
    } : current);
    setStatus(null);
    setResetArmed(false);
  }

  async function save() {
    if (!draft) return;
    setSaving(true);
    setError(null);
    setStatus(null);
    try {
      const next = await service.save(draft);
      setSaved(next);
      setDraft(promptDraftFromState(next));
      setPreview(next);
      setStatus("저장했습니다. 변경 내용은 새 대화부터 적용됩니다.");
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function resetDefaults() {
    setResetArmed(false);
    setSaving(true);
    setError(null);
    try {
      const next = await service.resetDefaults();
      setSaved(next);
      setDraft(promptDraftFromState(next));
      setPreview(next);
      setSelectedPath(next.documents[0]?.path ?? "");
      setStatus("기본 프롬프트로 복원했습니다.");
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setSaving(false);
    }
  }

  async function openPromptFolder() {
    if (!saved) return;
    setError(null);
    try {
      await openPath(saved.directoryPath);
    } catch (nextError) {
      setError(errorText(nextError));
    }
  }

  return <div id="settings-panel-persona" role="tabpanel" className="persona-settings">
    <section className="settings-section settings-section-first persona-summary">
      <div className="persona-heading">
        <div>
          <h3>페르소나</h3>
          <strong>{saved.name}</strong>
          <p>돌쇠의 핵심 원칙과 말투를 편집합니다. 저장한 내용은 새 대화부터 고정됩니다.</p>
        </div>
        <label className="switch-control">
          <input
            type="checkbox"
            aria-label="페르소나 사용"
            checked={draft.enabled}
            onChange={(event) => {
              setDraft({ ...draft, enabled: event.target.checked });
              setStatus(null);
            }}
          />
          <span aria-hidden="true" />
          <strong>{draft.enabled ? "사용" : "사용 안 함"}</strong>
        </label>
      </div>
      <div className="persona-toolbar">
        <button className="button-secondary" type="button" onClick={() => void openPromptFolder()}>
          <FolderOpen size={14} aria-hidden="true" />폴더 열기
        </button>
        <button className="button-secondary" type="button" disabled={saving} onClick={() => void load()}>
          <RefreshCw size={14} aria-hidden="true" />다시 읽기
        </button>
        <button className="button-primary" type="button" disabled={!dirty || saving} onClick={() => void save()}>
          <Save size={14} aria-hidden="true" />{saving ? "저장 중..." : "저장"}
        </button>
      </div>
      {status && <p className="persona-status" role="status">{status}</p>}
      {error && <p className="persona-error" role="alert">{error}</p>}
    </section>

    <section className="settings-section persona-editor-section">
      <div className="persona-section-heading">
        <h3>프롬프트 문서</h3>
        <span>{selectedDraft?.content.length.toLocaleString("ko-KR") ?? 0}자</span>
      </div>
      <div className="persona-document-tabs" role="tablist" aria-label="프롬프트 문서">
        {saved.documents.map((document) => <button
          key={document.path}
          type="button"
          role="tab"
          aria-selected={selected?.path === document.path}
          className={selected?.path === document.path ? "selected" : ""}
          onClick={() => setSelectedPath(document.path)}
        >
          <strong>{document.label}</strong>
          <span>{document.path}</span>
        </button>)}
      </div>
      {selected && <div className="persona-editor">
        <label htmlFor="persona-document-editor">{selected.description}</label>
        <textarea
          id="persona-document-editor"
          spellCheck={false}
          value={selectedDraft?.content ?? ""}
          onChange={(event) => updateDocument(event.target.value)}
        />
      </div>}
    </section>

    <section className="settings-section persona-preview-section">
      <div className="persona-section-heading">
        <div>
          <h3><Eye size={14} aria-hidden="true" />컴파일된 시스템 프롬프트</h3>
          <p>모델에는 아래 내용이 하나의 system 메시지로 전달됩니다.</p>
        </div>
        <span>{preview?.characterCount.toLocaleString("ko-KR") ?? 0}자 · 약 {preview?.estimatedTokens.toLocaleString("ko-KR") ?? 0} 토큰</span>
      </div>
      <pre className="persona-compiled-preview">{draft.enabled
        ? preview?.compiledPrompt || "미리보기를 준비하는 중..."
        : "페르소나가 꺼져 있어 system 메시지를 추가하지 않습니다."}</pre>
      <div className="persona-reset-row">
        {resetArmed
          ? <>
              <span>편집한 내용을 버리고 기본 프롬프트로 복원할까요?</span>
              <button className="button-secondary" type="button" onClick={() => setResetArmed(false)}>취소</button>
              <button className="button-danger" type="button" onClick={() => void resetDefaults()}>기본값 복원</button>
            </>
          : <button className="button-secondary" type="button" onClick={() => setResetArmed(true)}>
              <RotateCcw size={14} aria-hidden="true" />기본값 복원
            </button>}
      </div>
    </section>
  </div>;
}
