import type { Message, RuntimeTelemetry } from "./runtime";
import type { LlmEventDto, LlmMetricsDto, LlmPhase, LlmStatusDto } from "./nativeRuntime";

export interface NativeState {
  phase: LlmPhase;
  modelName: string;
  modelPath: string | null;
  backend: string;
  messages: Message[];
  activeRequestHandle: string | null;
  pendingSubmit: boolean;
  loadingProgress: number | null;
  error: string | null;
  telemetry: RuntimeTelemetry;
  nextMessageId: number;
}

export type NativeAction =
  | { type: "status"; status: LlmStatusDto }
  | { type: "load-started"; modelPath: string }
  | { type: "load-failed"; error: string }
  | { type: "submit-started"; prompt: string }
  | { type: "submit-accepted"; requestHandle: string }
  | { type: "submit-failed"; error: string }
  | { type: "token"; requestHandle: string; text: string }
  | { type: "terminal"; requestHandle: string; status: "complete" | "cancelled" | "error"; tail: string; error?: string }
  | { type: "metrics"; metrics: LlmMetricsDto }
  | { type: "progress"; progress: number }
  | { type: "reset" };

const emptyTelemetry: RuntimeTelemetry = {
  backend: "—",
  speed: "—",
  tokens: "—",
  context: "—",
  elapsed: "—",
};

export function createNativeState(): NativeState {
  return {
    phase: "no-model",
    modelName: "GGUF 모델 선택",
    modelPath: null,
    backend: "CPU",
    messages: [],
    activeRequestHandle: null,
    pendingSubmit: false,
    loadingProgress: null,
    error: null,
    telemetry: emptyTelemetry,
    nextMessageId: 1,
  };
}

function updateAssistant(state: NativeState, update: (message: Message) => Message): Message[] {
  let index = -1;
  for (let current = state.messages.length - 1; current >= 0; current -= 1) {
    if (state.messages[current].role === "assistant") {
      index = current;
      break;
    }
  }
  if (index < 0) return state.messages;
  return state.messages.map((message, current) => current === index ? update(message) : message);
}

function acceptsHandle(state: NativeState, requestHandle: string): boolean {
  return state.activeRequestHandle === requestHandle || (state.pendingSubmit && state.activeRequestHandle === null);
}

export function nativeReducer(state: NativeState, action: NativeAction): NativeState {
  switch (action.type) {
    case "status":
      return {
        ...state,
        phase: action.status.phase,
        modelName: action.status.modelName ?? "GGUF 모델 선택",
        modelPath: action.status.modelPath,
        backend: action.status.backend ?? "CPU",
        activeRequestHandle: action.status.activeRequestHandle,
        loadingProgress: action.status.loadingProgress,
        error: action.status.lastError,
        telemetry: action.status.phase === "no-model" ? emptyTelemetry : { ...state.telemetry, backend: action.status.backend ?? "CPU" },
      };
    case "load-started": {
      const parts = action.modelPath.split(/[\\/]/);
      return { ...state, phase: "loading", modelPath: action.modelPath, modelName: parts[parts.length - 1] || action.modelPath, loadingProgress: 0, error: null };
    }
    case "load-failed":
      return { ...state, phase: "error", error: action.error, loadingProgress: null };
    case "submit-started": {
      if (state.phase !== "ready") return state;
      const id = state.nextMessageId;
      return {
        ...state,
        phase: "streaming",
        pendingSubmit: true,
        activeRequestHandle: null,
        error: null,
        nextMessageId: id + 2,
        messages: [
          ...state.messages,
          { id: `native-${id}`, role: "user", content: action.prompt, time: "방금" },
          { id: `native-${id + 1}`, role: "assistant", content: "", status: "streaming" },
        ],
      };
    }
    case "submit-accepted":
      if (!state.pendingSubmit || (state.activeRequestHandle && state.activeRequestHandle !== action.requestHandle)) {
        return state;
      }
      return { ...state, activeRequestHandle: action.requestHandle, pendingSubmit: false };
    case "submit-failed":
      return {
        ...state,
        phase: "error",
        activeRequestHandle: null,
        pendingSubmit: false,
        error: action.error,
        messages: updateAssistant(state, (message) => ({ ...message, status: "error" })),
      };
    case "token":
      if (!acceptsHandle(state, action.requestHandle)) return state;
      return {
        ...state,
        activeRequestHandle: action.requestHandle,
        pendingSubmit: false,
        messages: updateAssistant(state, (message) => ({ ...message, content: message.content + action.text, status: "streaming" })),
      };
    case "terminal":
      if (!acceptsHandle(state, action.requestHandle)) return state;
      return {
        ...state,
        phase: action.status === "error" ? "error" : "ready",
        activeRequestHandle: null,
        pendingSubmit: false,
        error: action.error ?? null,
        messages: updateAssistant(state, (message) => ({ ...message, content: message.content + action.tail, status: action.status })),
      };
    case "metrics": {
      const generated = Number(action.metrics.generatedTokens);
      const elapsedSeconds = Number(action.metrics.decodeNanoseconds) / 1_000_000_000;
      return {
        ...state,
        telemetry: {
          backend: state.backend,
          speed: `${action.metrics.tokensPerSecond.toFixed(1)} tok/s`,
          tokens: `프롬프트 ${action.metrics.promptTokens} / 생성 ${action.metrics.generatedTokens}`,
          context: "—",
          elapsed: Number.isFinite(elapsedSeconds) ? `${elapsedSeconds.toFixed(1)}s` : "—",
        },
        messages: state.phase === "streaming" && Number.isFinite(generated)
          ? updateAssistant(state, (message) => ({ ...message, metrics: `${action.metrics.tokensPerSecond.toFixed(1)} tok/s · 생성 ${generated} 토큰` }))
          : state.messages,
      };
    }
    case "progress":
      return state.phase === "loading" ? { ...state, loadingProgress: action.progress } : state;
    case "reset":
      return { ...state, messages: [], activeRequestHandle: null, pendingSubmit: false, phase: state.modelPath ? "ready" : "no-model", error: null };
  }
}

export class TokenDecoders {
  private readonly values = new Map<string, TextDecoder>();

  push(requestHandle: string, bytes: number[]): string {
    let decoder = this.values.get(requestHandle);
    if (!decoder) {
      decoder = new TextDecoder();
      this.values.set(requestHandle, decoder);
    }
    return decoder.decode(Uint8Array.from(bytes), { stream: true });
  }

  finish(requestHandle: string): string {
    const decoder = this.values.get(requestHandle);
    if (!decoder) return "";
    this.values.delete(requestHandle);
    return decoder.decode();
  }

  remove(requestHandle: string): void {
    this.values.delete(requestHandle);
  }

  clear(): void {
    this.values.clear();
  }
}

function parseProgress(bytes: number[]): number | null {
  try {
    const payload = JSON.parse(new TextDecoder().decode(Uint8Array.from(bytes))) as { progress?: unknown };
    return typeof payload.progress === "number" ? Math.min(1, Math.max(0, payload.progress)) : null;
  } catch {
    return null;
  }
}

export function applyNativeEvent(state: NativeState, event: LlmEventDto, decoders: TokenDecoders): NativeState {
  if (event.kind === "model-progress") {
    const progress = parseProgress(event.bytes);
    return progress === null ? state : nativeReducer(state, { type: "progress", progress });
  }
  if (event.kind === "metrics" && event.metrics) {
    return nativeReducer(state, { type: "metrics", metrics: event.metrics });
  }
  const handle = event.requestHandle;
  if (!handle) return state;
  if (event.kind === "queued") {
    return state.pendingSubmit && state.activeRequestHandle === null
      ? { ...state, activeRequestHandle: handle }
      : state;
  }
  if (event.kind === "token") {
    return nativeReducer(state, { type: "token", requestHandle: handle, text: decoders.push(handle, event.bytes) });
  }
  if (event.kind === "done" || event.kind === "cancelled" || event.kind === "error") {
    const tail = decoders.finish(handle);
    const status = event.kind === "done" ? "complete" : event.kind === "cancelled" ? "cancelled" : "error";
    return nativeReducer(state, {
      type: "terminal",
      requestHandle: handle,
      status,
      tail,
      error: event.kind === "error" ? `생성 오류 (${event.errorCode})` : undefined,
    });
  }
  return state;
}
