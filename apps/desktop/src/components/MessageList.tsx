import { Box, BookOpen, Download, FolderOpen, LoaderCircle, Square, TriangleAlert } from "lucide-react";
import type { Message, MockStateName } from "../services/runtime";

interface EmptyContentProps {
  state: MockStateName;
  modelName?: string;
  backend?: string;
  loadingProgress?: number | null;
  onChooseModel?(): void;
  onCancelLoad?(): void;
}

function EmptyContent({ state, modelName, backend, loadingProgress, onChooseModel, onCancelLoad }: EmptyContentProps) {
  if (state === "no-model") return <div className="empty-state"><Box size={28} strokeWidth={1.5} /><h2>선택된 모델이 없습니다</h2><p>로컬 GGUF 파일을 선택하면 대화를 시작할 수 있습니다. 모든 추론은 이 PC에서만 실행됩니다.</p><button className="button-primary" type="button" onClick={onChooseModel}><FolderOpen size={14} /> GGUF 모델 선택…</button><span className="caption">지원 형식: .gguf</span></div>;
  if (state === "loading") { const progress = Math.round((loadingProgress ?? 0.64) * 100); return <div className="empty-state"><div className="loading-card"><p className="loading-file">{modelName ?? "Qwen2.5-7B-Instruct-Q4_K_M.gguf"}</p><div className="progress-track"><div className="progress-fill" style={{ width: `${progress}%` }} /></div><div className="loading-meta"><span>모델 로딩 중 · {backend ?? "CUDA"}</span><span>{progress}%</span></div>{(onCancelLoad || loadingProgress === undefined) && <button className="button-secondary" type="button" onClick={onCancelLoad}>로드 취소</button>}</div></div>; }
  return <div className="empty-state"><BookOpen size={28} strokeWidth={1.5} /><h2>새 대화를 시작하세요</h2><p>메시지를 입력하면 이 PC에서 바로 추론이 실행됩니다. 대화 내용은 외부로 전송되지 않습니다.</p><span className="ready-line"><span className="status-dot ready" />{modelName ?? "Qwen2.5-7B-Instruct"} · {backend ?? "CUDA"} · 준비됨</span></div>;
}

interface MessageListProps extends EmptyContentProps {
  messages: Message[];
  error?: string | null;
  onOpenSettings?(): void;
}

export function shouldShowEmptyContent(state: MockStateName, messageCount: number): boolean {
  if (state === "loading") return true;
  if (state === "no-model") return messageCount === 0;
  return state === "empty" && messageCount === 0;
}

export function isCpuRuntimeRecoveryError(error?: string | null): boolean {
  return Boolean(error && (
    error.includes("CPU runtime is unavailable")
    || error.includes("CPU runtime recovery failed")
  ));
}

export function MessageList({ state, messages, modelName, backend, loadingProgress, onChooseModel, onCancelLoad, onOpenSettings, error }: MessageListProps) {
  if (shouldShowEmptyContent(state, messages.length)) return <div className="message-column"><EmptyContent state={state} modelName={modelName} backend={backend} loadingProgress={loadingProgress} onChooseModel={onChooseModel} onCancelLoad={onCancelLoad} /></div>;
  if (state === "error" && isCpuRuntimeRecoveryError(error)) return <div className="message-column"><div className="message assistant"><div className="error-block"><h3 className="error-title"><TriangleAlert size={14} />CPU 런타임이 필요합니다</h3><p>기본 CPU 런타임을 불러오지 못했습니다. 설정에서 검증된 CPU 런타임을 설치한 뒤 앱을 재시작하세요.</p><div className="error-actions"><button className="button-primary" type="button" onClick={onOpenSettings}><Download size={14} /> CPU 런타임 설치</button></div></div></div></div>;
  if (state === "error" && error) return <div className="message-column"><div className="message assistant"><div className="error-block"><h3 className="error-title"><TriangleAlert size={14} />로컬 추론을 완료하지 못했습니다</h3><p>{error}</p><div className="error-actions"><button className="button-secondary" type="button" onClick={onChooseModel}>모델 다시 선택</button></div></div></div></div>;
  if (state === "error") return <div className="message-column"><div className="message user"><div className="user-bubble">GGUF 양자화 방식 중 Q4_K_M과 Q5_K_M의 차이를 설명해줘. 7B 모델을 12GB VRAM에서 돌릴 건데 어느 쪽이 나아?</div><div className="timestamp">14:02</div></div><div className="message assistant"><div className="error-block"><h3 className="error-title"><TriangleAlert size={14} />CUDA 백엔드를 초기화하지 못했습니다</h3><p>NVIDIA 드라이버에서 CUDA 12 런타임을 찾을 수 없습니다 (오류 코드 LLW_E_BACKEND_INIT). 드라이버를 업데이트하거나 CPU 런타임으로 전환한 뒤 다시 시도하세요.</p><div className="error-actions"><button className="button-secondary" type="button">CPU로 전환 후 다시 로드</button><button className="button-secondary" type="button">다시 시도</button></div></div></div></div>;
  return (
    <div className="message-column">
      {messages.map((message) => message.role === "user" ? (
        <div className="message user" data-message-role="user" key={message.id}><div className="user-bubble">{message.content}</div>{message.time && <div className="timestamp">{message.time}</div>}</div>
      ) : (
        <div className="message assistant" data-long-message={message.id === "long" || undefined} key={message.id}>{message.content}{message.status === "streaming" && <span className="streaming-cursor" />}{message.status === "cancelled" && <div className="stopped-line"><Square size={12} fill="currentColor" />생성이 중지되었습니다 · {message.stopDetail ?? "토큰 87개 생성됨"}</div>}{message.status === "interrupted" && <div className="stopped-line"><Square size={12} fill="currentColor" />생성이 중단되었습니다 · {message.stopDetail ?? "토큰 87개 생성됨"}</div>}{message.metrics && <div className={`metrics-line ${message.status === "streaming" ? "live" : ""}`}>{message.metrics}</div>}</div>
      ))}
      {["streaming", "multi"].includes(state) && <span className="sr-only"><LoaderCircle />{state === "multi" ? "생성 중 · 2" : "생성 중"}</span>}
    </div>
  );
}
