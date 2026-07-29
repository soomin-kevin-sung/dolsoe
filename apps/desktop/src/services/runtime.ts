import type { AgentRunTrace } from "./conversationService";

export const mockStates = [
  "no-model", "loading", "empty", "ready", "streaming", "cancelled", "error",
  "multi", "settings", "reset-confirm", "reload-confirm", "pack-install",
  "diagnostics", "interrupted",
] as const;

export type MockStateName = (typeof mockStates)[number];
export type ThemePreference = "light" | "dark" | "system";
export type RuntimeStatus = "none" | "loading" | "pending" | "ready" | "streaming" | "error";

export interface Session {
  id: string;
  title: string;
  meta: string;
  active?: boolean;
  generating?: boolean;
  queued?: boolean;
}

export interface Message {
  id: string;
  role: "user" | "assistant";
  content: string;
  time?: string;
  status?: "complete" | "streaming" | "cancelled" | "interrupted" | "error";
  metrics?: string;
  stopDetail?: string;
  agentRun?: AgentRunTrace;
}

export interface RuntimeTelemetry {
  backend: string;
  speed: string;
  tokens: string;
  context: string;
  elapsed: string;
}

export interface RuntimePack {
  id: "cpu" | "cuda" | "vulkan";
  name: string;
  version: string;
  status: "installed" | "available" | "installing";
  progress?: number;
}

export interface RuntimeSnapshot {
  state: MockStateName;
  title: string;
  modelName: string;
  runtimeStatus: RuntimeStatus;
  statusText: string;
  sessions: Session[];
  messages: Message[];
  telemetry: RuntimeTelemetry;
  packs: RuntimePack[];
  settingsOpen: boolean;
  diagnosticsOpen: boolean;
  dialog: "reset" | "reload" | null;
}

export interface RuntimeService {
  getSnapshot(state: MockStateName): RuntimeSnapshot;
  subscribe(listener: (snapshot: RuntimeSnapshot) => void): () => void;
  sendPrompt(sessionId: string, prompt: string): Promise<void>;
  cancel(sessionId: string): Promise<void>;
  resetSession(sessionId: string): Promise<void>;
}

export function parseMockState(value: string | null): MockStateName {
  return mockStates.includes(value as MockStateName) ? (value as MockStateName) : "ready";
}
