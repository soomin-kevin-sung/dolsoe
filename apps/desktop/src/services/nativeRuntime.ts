import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

export type LlmPhase = "no-model" | "loading" | "ready" | "streaming" | "error";
export type LlmEventKind = "model-progress" | "queued" | "token" | "metrics" | "done" | "cancelled" | "error";

export interface LlmStatusDto {
  phase: LlmPhase;
  runtimePackId: string | null;
  modelPath: string | null;
  modelName: string | null;
  backend: string | null;
  loadingProgress: number | null;
  activeRequestHandle: string | null;
  lastError: string | null;
}

export interface LoadModelRequest {
  runtimePackId: string;
  backend: "cpu" | "cuda" | "vulkan";
  deviceIndex: number;
  modelPath: string;
  contextSize: number;
  batchSize: number;
  physicalBatchSize: number;
  threads: number;
  useMmap: boolean;
}

export interface SubmitRequest {
  conversationId: string;
  agentRunId: string;
  agentStepId: string;
  prompt: string;
  messages: Array<{ role: "system" | "user" | "assistant"; content: string }>;
  maxNewTokens: number;
  temperature: number;
  topK: number;
  topP: number;
  minP: number;
  repeatLastN: number;
  repeatPenalty: number;
  frequencyPenalty: number;
  presencePenalty: number;
  stopSequences: string[];
  seed: number;
}

export interface SubmitResponse {
  requestHandle: string;
}

export interface LlmMetricsDto {
  promptTokens: string;
  generatedTokens: string;
  cancelledRequests: string;
  failedRequests: string;
  queueWaitNanoseconds: string;
  decodeNanoseconds: string;
  tokensPerSecond: number;
}

export interface LlmEventDto {
  kind: LlmEventKind;
  requestHandle: string | null;
  correlationId: string | null;
  sequenceNumber: string;
  bytes: number[];
  errorCode: number;
  metrics: LlmMetricsDto | null;
}

export type AgentActivityKind =
  | "thinking"
  | "choosing-tool"
  | "tool-started"
  | "tool-completed"
  | "tool-failed"
  | "writing"
  | "answer-reset"
  | "completed"
  | "cancelled"
  | "failed";

export interface AgentActivityEventDto {
  kind: AgentActivityKind;
  runId: string;
  conversationId: string;
  assistantMessageId: string;
  activityId?: string;
  toolName?: string;
  input?: string;
  output?: string;
  durationMs?: number;
}

export interface NativeBindings {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (event: { payload: T }) => void): Promise<() => void>;
  openGguf(): Promise<string | null>;
}

export const tauriBindings: NativeBindings = {
  invoke,
  listen: async <T>(event: string, handler: (event: { payload: T }) => void) => listen<T>(event, handler),
  openGguf: async () => {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [{ name: "GGUF 모델", extensions: ["gguf"] }],
    });
    return typeof selected === "string" ? selected : null;
  },
};

export class NativeRuntimeService {
  constructor(private readonly bindings: NativeBindings = tauriBindings) {}

  getStatus(): Promise<LlmStatusDto> {
    return this.bindings.invoke("llm_get_status");
  }

  loadModel(request: LoadModelRequest): Promise<LlmStatusDto> {
    return this.bindings.invoke("llm_load_model", { request });
  }

  unloadModel(): Promise<LlmStatusDto> {
    return this.bindings.invoke("llm_unload_model");
  }

  submit(request: SubmitRequest): Promise<SubmitResponse> {
    return this.bindings.invoke("llm_submit", { request });
  }

  cancel(requestHandle: string): Promise<void> {
    return this.bindings.invoke("llm_cancel", { requestHandle });
  }

  getMetrics(): Promise<LlmMetricsDto> {
    return this.bindings.invoke("llm_get_metrics");
  }

  subscribe(listener: (event: LlmEventDto) => void): Promise<() => void> {
    return this.bindings.listen<LlmEventDto>("llm://event", (event) => listener(event.payload));
  }

  subscribeAgentActivity(listener: (event: AgentActivityEventDto) => void): Promise<() => void> {
    return this.bindings.listen<AgentActivityEventDto>(
      "agent://activity",
      (event) => listener(event.payload),
    );
  }

  subscribeHostReady(listener: (status: LlmStatusDto) => void): Promise<() => void> {
    return this.bindings.listen<LlmStatusDto>("llm://host-ready", (event) => listener(event.payload));
  }

  chooseModel(): Promise<string | null> {
    return this.bindings.openGguf();
  }
}
