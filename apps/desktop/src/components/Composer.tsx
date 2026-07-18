import { ArrowUp, Square } from "lucide-react";
import { useState } from "react";
import type { MockStateName } from "../services/runtime";

const copy: Partial<Record<MockStateName, [string, string]>> = {
  "no-model": ["모델을 선택하면 대화를 시작할 수 있습니다", "GGUF 모델을 먼저 선택하세요"],
  loading: ["모델 로딩이 끝나면 보낼 수 있습니다", "모델을 메모리에 올리는 중입니다"],
  error: ["모델이 로드되지 않았습니다", "CPU로 전환 후 다시 로드하면 대화할 수 있습니다"],
  streaming: ["메시지를 입력하세요", "생성이 끝나면 보낼 수 있습니다 · Esc 생성 중지"],
  multi: ["메시지를 입력하세요", "생성이 끝나면 보낼 수 있습니다 · Esc 생성 중지"],
};

export function Composer({ disabled, streaming, state, onSend }: { disabled: boolean; streaming: boolean; state: MockStateName; onSend(prompt: string): void }) {
  const [draft, setDraft] = useState(state === "ready" ? "그럼 속도가 제일 빠른 양자화는 뭐야?" : "");
  const [placeholder, hint] = copy[state] ?? ["메시지를 입력하세요", "Enter 전송 · Shift+Enter 줄바꿈"];
  function submit() { const value = draft.trim(); if (!value || disabled || streaming) return; onSend(value); setDraft(""); }
  return <div className="composer-area"><div className="composer-inner"><form className={`composer ${disabled ? "disabled" : ""}`} aria-label="메시지 입력" onSubmit={(event) => { event.preventDefault(); submit(); }}><textarea aria-label="메시지" rows={1} disabled={disabled || streaming} placeholder={placeholder} value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); submit(); } }} />{streaming ? <button type="button" className="send-button stop-button" aria-label="생성 중지" title="생성 중지"><Square size={12} fill="currentColor" /></button> : <button type="submit" className="send-button" disabled={disabled || !draft.trim()} aria-label="전송" title="전송"><ArrowUp size={16} /></button>}</form><div className="composer-hint">{hint}</div></div></div>;
}
