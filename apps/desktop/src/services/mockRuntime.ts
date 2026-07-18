import type {
  Message,
  MockStateName,
  RuntimeService,
  RuntimeSnapshot,
  Session,
} from "./runtime";

export const DEFAULT_MODEL = "Qwen2.5-7B-Instruct-Q4_K_M.gguf";

const ASSIST_A1 = "Q4_K_M과 Q5_K_M은 모두 k-quant 계열의 혼합 정밀도 양자화입니다. 핵심 차이는 가중치당 평균 비트 수입니다.\n\nQ4_K_M은 평균 약 4.8bpw로 7B 모델 기준 파일 크기가 약 4.4GB이고, Q5_K_M은 평균 약 5.7bpw로 약 5.1GB입니다. 품질은 Q5_K_M이 퍼플렉시티 기준으로 조금 더 원본에 가깝지만, 실사용 체감 차이는 크지 않습니다.\n\n12GB VRAM이라면 두 방식 모두 전체 레이어를 GPU에 올릴 수 있습니다. 컨텍스트를 8K 이상 길게 쓸 계획이면 KV 캐시 여유가 있는 Q4_K_M을, 품질을 조금이라도 높이고 싶다면 Q5_K_M을 권합니다.";
const ASSIST_A2_FULL = "KV 캐시 크기는 대략 2 × 레이어 수 × 컨텍스트 길이 × KV 헤드 수 × 헤드 차원 × 정밀도(byte)로 계산합니다.\n\nQwen2.5-7B는 28개 레이어, GQA 구조로 KV 헤드가 4개입니다. FP16 KV 캐시 기준으로 8K 컨텍스트는 약 0.9GB, 16K는 약 1.8GB를 사용합니다. 즉 16K로 늘리면 KV 캐시가 약 0.9GB 더 필요합니다.\n\n모델 가중치 4.4GB에 KV 캐시 1.8GB, 연산 버퍼를 더해도 12GB 안에 여유 있게 들어갑니다.";
const ASSIST_A2_PARTIAL = "KV 캐시 크기는 대략 2 × 레이어 수 × 컨텍스트 길이 × KV 헤드 수 × 헤드 차원 × 정밀도(byte)로 계산합니다.\n\nQwen2.5-7B는 28개 레이어, GQA 구조로 KV 헤드가 4개입니다. FP16 KV 캐시 기준으로 8K 컨텍스트는 약 0.9GB,";

const sessions: Session[] = [
  { id: "quant", title: "GGUF 양자화 비교", meta: "14:06", active: true },
  { id: "build", title: "llama.cpp 빌드 오류 분석", meta: "어제" },
  { id: "cuda", title: "CUDA 오프로딩 설정 정리", meta: "2일 전" },
  { id: "meeting", title: "주간 회의록 요약", meta: "7월 11일" },
];

const messages: Message[] = [
  {
    id: "q1",
    role: "user",
    content: "GGUF 양자화 방식 중 Q4_K_M과 Q5_K_M의 차이를 설명해줘. 7B 모델을 12GB VRAM에서 돌릴 건데 어느 쪽이 나아?",
    time: "14:02",
  },
  {
    id: "a1",
    role: "assistant",
    content: ASSIST_A1,
    status: "complete",
    metrics: "41.8 tok/s · 프롬프트 96 · 생성 187 토큰 · 4.5s",
  },
];

function snapshot(state: MockStateName): RuntimeSnapshot {
  const streaming = state === "streaming" || state === "multi";
  const stateMessages = [...messages];
  if (["ready", "streaming", "multi", "cancelled", "interrupted"].includes(state)) {
    stateMessages.push({
      id: "q2",
      role: "user",
      content: "그럼 Q4_K_M 기준으로 컨텍스트를 16K까지 늘리면 VRAM이 얼마나 더 필요해?",
      time: "14:06",
    });
    stateMessages.push({
      id: "a2",
      role: "assistant",
      content: streaming || state === "cancelled" || state === "interrupted" ? ASSIST_A2_PARTIAL : ASSIST_A2_FULL,
      status: streaming ? "streaming" : state === "cancelled" ? "cancelled" : state === "interrupted" ? "interrupted" : "complete",
      metrics: streaming ? "38.6 tok/s · 프롬프트 214 · 생성 118 토큰 · 3.1s" : "42.1 tok/s · 프롬프트 214 · 생성 164 토큰 · 3.9s",
    });
  }

  const modelMissing = state === "no-model";
  const modelLoading = state === "loading";
  const hasError = state === "error";
  return {
    state,
    title: ["no-model", "loading", "empty"].includes(state) ? "새 대화" : state === "diagnostics" ? "진단" : "GGUF 양자화 비교",
    modelName: modelMissing ? "GGUF 모델 선택" : DEFAULT_MODEL,
    runtimeStatus: modelMissing ? "none" : modelLoading ? "loading" : hasError ? "error" : streaming ? "streaming" : "ready",
    statusText: modelMissing ? "모델 없음" : modelLoading ? "모델 로딩 중" : hasError ? "백엔드 오류" : streaming ? state === "multi" ? "생성 중 · 2" : "생성 중" : "준비됨",
    sessions: state === "multi"
      ? sessions.map((session, index) => ({ ...session, generating: index < 2, queued: index === 2 }))
      : state === "streaming"
        ? sessions.map((session, index) => ({ ...session, generating: index === 0 }))
        : sessions,
    messages: state === "empty" || modelMissing || modelLoading ? [] : stateMessages,
    telemetry: {
      backend: modelMissing ? "—" : "CUDA · RTX 4070",
      speed: modelMissing ? "—" : streaming ? "38.6 tok/s" : hasError ? "—" : "42.1 tok/s",
      tokens: modelMissing ? "—" : streaming ? "프롬프트 214 / 생성 118" : "프롬프트 214 / 생성 164",
      context: modelMissing ? "—" : "1,847 / 8,192",
      elapsed: modelMissing ? "—" : streaming ? "3.1s" : hasError ? "—" : "3.9s",
    },
    packs: [
      { id: "cpu", name: "CPU", version: "2026.07.1", status: "installed" },
      { id: "cuda", name: "CUDA", version: "2026.07.1", status: state === "pack-install" ? "installing" : "installed", progress: state === "pack-install" ? 64 : undefined },
      { id: "vulkan", name: "Vulkan", version: "2026.07.1", status: "available" },
    ],
    settingsOpen: ["settings", "reload-confirm", "pack-install"].includes(state),
    diagnosticsOpen: state === "diagnostics",
    dialog: state === "reset-confirm" ? "reset" : state === "reload-confirm" ? "reload" : null,
  };
}

export class MockRuntimeService implements RuntimeService {
  private listeners = new Set<(value: RuntimeSnapshot) => void>();

  getSnapshot(state: MockStateName): RuntimeSnapshot {
    return snapshot(state);
  }

  subscribe(listener: (value: RuntimeSnapshot) => void): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  async sendPrompt(_sessionId: string, _prompt: string): Promise<void> {}
  async cancel(_sessionId: string): Promise<void> {}
  async resetSession(_sessionId: string): Promise<void> {}
}

export const mockRuntime = new MockRuntimeService();
