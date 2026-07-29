import { Check, Clipboard, RefreshCw, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import {
  PersonaPromptService,
  type ConversationPromptPreview,
} from "../services/personaPrompts";
import { IconButton } from "./IconButton";

function errorText(error: unknown) {
  return error instanceof Error ? error.message : String(error);
}

interface Props {
  open: boolean;
  conversationId: string | null;
  conversationTitle: string;
  onClose(): void;
}

type InspectorView = "final" | "messages";

export function PromptInspector({ open, conversationId, conversationTitle, onClose }: Props) {
  const service = useMemo(() => new PersonaPromptService(), []);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [preview, setPreview] = useState<ConversationPromptPreview | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [copied, setCopied] = useState(false);
  const [view, setView] = useState<InspectorView>("final");

  async function refresh() {
    if (!conversationId) return;
    setLoading(true);
    setError(null);
    try {
      setPreview(await service.previewConversation(conversationId));
    } catch (nextError) {
      setError(errorText(nextError));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    if (!open || !conversationId) return;
    setPreview(null);
    setCopied(false);
    setView("final");
    void refresh();
    const focusFrame = requestAnimationFrame(() => closeRef.current?.focus());
    return () => cancelAnimationFrame(focusFrame);
  }, [conversationId, open]);

  useEffect(() => {
    if (!open) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [onClose, open]);

  if (!open || !conversationId) return null;

  const activePrompt = preview
    ? view === "final" ? preview.finalPrompt ?? "" : preview.structuredPrompt
    : "";
  const activeBytes = new TextEncoder().encode(activePrompt).length;
  const activeCharacters = Array.from(activePrompt).length;
  const activeEstimatedTokens = activePrompt ? Math.ceil(activeBytes / 4) : 0;

  async function copyPrompt() {
    if (!activePrompt) return;
    await navigator.clipboard.writeText(activePrompt);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  }

  return <div className="prompt-inspector-scrim" onMouseDown={(event) => {
    if (event.target === event.currentTarget) onClose();
  }}>
    <section className="prompt-inspector" role="dialog" aria-modal="true" aria-labelledby="prompt-inspector-title">
      <header className="prompt-inspector-header">
        <div>
          <h2 id="prompt-inspector-title">모델 입력 검사기</h2>
          <p>{conversationTitle}</p>
        </div>
        <IconButton buttonRef={closeRef} icon={X} label="모델 입력 검사기 닫기" onClick={onClose} />
      </header>
      <div className="prompt-inspector-meta">
        <span>{preview
          ? preview.source === "conversation-snapshot" ? "대화 스냅샷" : "활성 페르소나"
          : "확인 중"}</span>
        <span>{preview?.messages.length ?? 0}개 메시지</span>
        <span>{activeCharacters.toLocaleString("ko-KR")}자</span>
        <span>약 {activeEstimatedTokens.toLocaleString("ko-KR")} 토큰</span>
        {preview && <code>{preview.personaId} · {preview.revision.slice(0, 10)}</code>}
      </div>
      <div className="prompt-inspector-tabs" role="tablist" aria-label="모델 입력 표시 방식">
        <button type="button" role="tab" aria-selected={view === "final"} className={view === "final" ? "selected" : ""} onClick={() => setView("final")}>
          최종 모델 입력
        </button>
        <button type="button" role="tab" aria-selected={view === "messages"} className={view === "messages" ? "selected" : ""} onClick={() => setView("messages")}>
          구조화 메시지
        </button>
      </div>
      <p className="prompt-inspector-note">
        {view === "final"
          ? "현재 로드된 GGUF의 채팅 템플릿과 assistant generation prompt까지 적용된 토큰화 직전 문자열입니다."
          : "GGUF 채팅 템플릿을 적용하기 전, 런타임에 전달되는 역할별 구조화 메시지입니다."}
      </p>
      <div className="prompt-inspector-body">
        {loading && <div className="prompt-inspector-state">현재 대화 컨텍스트를 조립하는 중...</div>}
        {error && <div className="prompt-inspector-state error" role="alert">{error}</div>}
        {!loading && !error && preview && view === "final" && preview.finalPromptError
          ? <div className="prompt-inspector-state error" role="alert">{preview.finalPromptError}</div>
          : null}
        {!loading && !error && preview && !(view === "final" && preview.finalPromptError)
          ? <pre>{activePrompt || "전달할 메시지가 없습니다."}</pre>
          : null}
      </div>
      <footer className="prompt-inspector-footer">
        <button className="button-secondary" type="button" disabled={loading} onClick={() => void refresh()}>
          <RefreshCw size={14} aria-hidden="true" />새로고침
        </button>
        <button className="button-secondary" type="button" disabled={!activePrompt} onClick={() => void copyPrompt()}>
          {copied ? <Check size={14} aria-hidden="true" /> : <Clipboard size={14} aria-hidden="true" />}
          {copied ? "복사됨" : "전체 복사"}
        </button>
        <button className="button-primary" type="button" onClick={onClose}>닫기</button>
      </footer>
    </section>
  </div>;
}
