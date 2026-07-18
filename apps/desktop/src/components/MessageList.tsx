import { Box, BookOpen, FolderOpen, LoaderCircle, Square, TriangleAlert } from "lucide-react";
import type { Message, MockStateName } from "../services/runtime";

function EmptyContent({ state }: { state: MockStateName }) {
  if (state === "no-model") return <div className="empty-state"><Box size={28} strokeWidth={1.5} /><h2>선택한 모델이 없습니다</h2><p>로컬 GGUF 파일을 선택하면 대화를 시작할 수 있습니다. 모든 추론은 이 PC에서만 실행됩니다.</p><button className="button-secondary" type="button"><FolderOpen size={14} /> GGUF 모델 선택…</button></div>;
  if (state === "loading") return <div className="empty-state"><div className="loading-card"><p className="loading-file">Qwen2.5-7B-Instruct-Q4_K_M.gguf</p><div className="progress-track"><div className="progress-fill" /></div><div className="loading-meta"><span>모델 로딩 중 · CUDA</span><span>64% · 2.8 / 4.4 GB</span></div><button className="button-secondary" type="button">로드 취소</button></div></div>;
  return <div className="empty-state"><BookOpen size={28} strokeWidth={1.5} /><h2>새 대화를 시작하세요</h2><p>메시지를 입력하면 이 PC에서 바로 추론을 실행합니다. 대화 내용은 외부로 전송되지 않습니다.</p><span className="ready-line"><span className="status-dot ready" />Qwen2.5-7B-Instruct · CUDA · 준비됨</span></div>;
}

export function MessageList({ state, messages }: { state: MockStateName; messages: Message[] }) {
  if (["no-model", "loading"].includes(state) || (state === "empty" && messages.length === 0)) return <div className="message-column"><EmptyContent state={state} /></div>;
  if (state === "error") return <div className="message-column"><div className="message user"><div className="user-bubble">GGUF 양자화 방식 중 Q4_K_M과 Q5_K_M의 차이를 설명해줘.</div></div><div className="error-block"><h3 className="error-title"><TriangleAlert size={14} />CUDA 백엔드를 초기화하지 못했습니다</h3><p>NVIDIA 드라이버에서 CUDA 12 런타임을 찾을 수 없습니다 (오류 코드 LLW_E_BACKEND_INIT). 드라이버를 업데이트하거나 CPU 백엔드로 전환한 후 다시 시도하세요.</p><div className="error-actions"><button className="button-secondary" type="button">CPU로 전환 후 다시 로드</button><button className="button-secondary" type="button">다시 시도</button></div></div></div>;
  return (
    <div className="message-column">
      {messages.map((message) => message.role === "user" ? (
        <div className="message user" data-message-role="user" key={message.id}><div className="user-bubble">{message.content}</div>{message.time && <div className="timestamp">{message.time}</div>}</div>
      ) : (
        <div className="message assistant" data-long-message={message.id === "long" || undefined} key={message.id}>{message.content}{message.status === "streaming" && <span className="streaming-cursor" />}{message.status === "cancelled" && <div className="stopped-line"><Square size={12} fill="currentColor" />생성을 중지했습니다 · 토큰 87개 생성됨</div>}{message.status === "interrupted" && <div className="stopped-line"><Square size={12} fill="currentColor" />생성이 중단되었습니다 · 토큰 87개 생성됨</div>}{message.metrics && <div className={`metrics-line ${message.status === "streaming" ? "live" : ""}`}>{message.metrics}</div>}</div>
      ))}
      {["streaming", "multi"].includes(state) && <span className="sr-only"><LoaderCircle />{state === "multi" ? "생성 중 · 2" : "생성 중"}</span>}
    </div>
  );
}
