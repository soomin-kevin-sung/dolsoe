import { ArrowUp, LockKeyhole } from "lucide-react";
import { useState, type Ref } from "react";
import type { MockStateName } from "../services/runtime";

const copy: Partial<Record<MockStateName, [string, string]>> = {
  "no-model": ["모델을 선택하면 대화를 시작할 수 있습니다", "GGUF 모델을 먼저 선택하세요"],
  loading: ["모델 로딩이 끝나면 보낼 수 있습니다", "모델을 메모리에 올리는 중입니다"],
  error: ["모델이 로드되지 않았습니다", "CPU로 전환 후 다시 로드하면 대화할 수 있습니다"],
  streaming: ["메시지를 입력하세요", "생성이 끝나면 보낼 수 있습니다 · Esc 생성 중지"],
  multi: ["메시지를 입력하세요", "생성이 끝나면 보낼 수 있습니다 · Esc 생성 중지"],
};

export function Composer({ disabled, streaming, state, runtimeRecovery, inputRef, onSend, onStop }: { disabled: boolean; streaming: boolean; state: MockStateName; runtimeRecovery?: boolean; inputRef: Ref<HTMLTextAreaElement>; onSend(prompt: string): boolean | void | Promise<boolean | void>; onStop(): void }) {
  const [draft, setDraft] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [placeholder, hint] = runtimeRecovery
    ? ["런타임을 설치하면 대화를 시작할 수 있습니다", "설정에서 런타임을 설치하세요"]
    : copy[state] ?? ["메시지를 입력하세요", "Enter 전송 · Shift+Enter 줄바꿈"];
  async function submit() {
    const value = draft.trim();
    if (!value || disabled || streaming || submitting) return;
    setSubmitting(true);
    try {
      const sent = await onSend(value);
      if (sent !== false) setDraft("");
    } finally {
      setSubmitting(false);
    }
  }
  return <div className="composer-area"><div className="composer-inner"><form className={`composer ${disabled ? "disabled" : ""}`} aria-label="메시지 입력" onSubmit={(event) => { event.preventDefault(); void submit(); }}><textarea ref={inputRef} aria-label="메시지" rows={1} disabled={disabled || streaming || submitting} placeholder={placeholder} value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); void submit(); } }} />{streaming ? <button type="button" className="send-button stop-button" aria-label="생성 중지" title="생성 중지" onClick={onStop}><span className="stop-button-icon" aria-hidden="true" /></button> : <button type="submit" className="send-button" disabled={disabled || submitting || !draft.trim()} aria-label="전송" title="전송"><ArrowUp size={16} /></button>}</form><div className="composer-meta"><span className="composer-hint">{hint}</span><span className="composer-local-note"><LockKeyhole size={11} />이 기기에서 생성</span></div></div></div>;
}
