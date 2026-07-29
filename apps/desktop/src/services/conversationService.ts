import { invoke } from "@tauri-apps/api/core";
import type { AgentModeId } from "./agentModes";

export type MessageRole = "user" | "assistant";
export type MessageStatus = "complete" | "streaming" | "cancelled" | "interrupted" | "error";
export type TerminalMessageStatus = Exclude<MessageStatus, "streaming">;

export interface ConversationSummary {
  id: string;
  title: string;
  agentMode: AgentModeId;
  createdAt: number;
  updatedAt: number;
}

export interface StoredMessage {
  id: string;
  conversationId: string;
  role: MessageRole;
  content: string;
  status: MessageStatus;
  kind: "chat" | "agent-mode-change";
  source: "user" | "model" | "application";
  metadataJson: string | null;
  createdAt: number;
  updatedAt: number;
}

export interface AgentToolTrace {
  activityId: string;
  toolName: string;
  status: "prepared" | "running" | "complete" | "cancelled" | "interrupted" | "error";
  input: string;
  output: string;
  durationMs: number;
}

export interface AgentRunTrace {
  runId: string;
  assistantMessageId: string;
  mode: AgentModeId;
  status: "prepared" | "running" | "complete" | "cancelled" | "interrupted" | "error";
  startedAt: number;
  finishedAt: number | null;
  phase?: "thinking" | "choosing-tool" | "writing";
  tools: AgentToolTrace[];
}

export interface ConversationDetail extends ConversationSummary {
  messages: StoredMessage[];
  agentRuns: AgentRunTrace[];
}

export interface ConversationBootstrap {
  conversations: ConversationSummary[];
  selected: ConversationDetail | null;
}

export interface StartedTurn {
  conversation: ConversationSummary;
  user: StoredMessage;
  assistant: StoredMessage;
  agentRunId?: string;
  agentStepId?: string;
}

export interface ConversationBindings {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
}

export interface AgentPreferences {
  defaultMode: AgentModeId;
}

export const tauriConversationBindings: ConversationBindings = { invoke };

export class ConversationService {
  constructor(private readonly bindings: ConversationBindings = tauriConversationBindings) {}

  bootstrap(): Promise<ConversationBootstrap> {
    return this.bindings.invoke("conversation_bootstrap");
  }

  load(conversationId: string): Promise<ConversationDetail> {
    return this.bindings.invoke("conversation_load", { conversationId });
  }

  rename(conversationId: string, title: string): Promise<ConversationSummary> {
    return this.bindings.invoke("conversation_rename", { conversationId, title });
  }

  clear(conversationId: string): Promise<ConversationDetail> {
    return this.bindings.invoke("conversation_clear", { conversationId });
  }

  delete(conversationId: string): Promise<ConversationDetail | null> {
    return this.bindings.invoke("conversation_delete", { conversationId });
  }

  startNewTurn(prompt: string, agentMode: AgentModeId): Promise<StartedTurn> {
    return this.bindings.invoke("conversation_start_new_turn", { prompt, agentMode });
  }

  startTurn(conversationId: string, prompt: string): Promise<StartedTurn> {
    return this.bindings.invoke("conversation_start_turn", { conversationId, prompt });
  }

  getAgentPreferences(): Promise<AgentPreferences> {
    return this.bindings.invoke("agent_get_preferences");
  }

  setDefaultAgentMode(mode: AgentModeId): Promise<AgentPreferences> {
    return this.bindings.invoke("agent_set_default_mode", { mode });
  }

  setConversationAgentMode(
    conversationId: string,
    mode: AgentModeId,
  ): Promise<ConversationDetail> {
    return this.bindings.invoke("conversation_set_agent_mode", { conversationId, mode });
  }

  finishTurn(
    assistantMessageId: string,
    content: string,
    status: TerminalMessageStatus,
  ): Promise<boolean> {
    return this.bindings.invoke("conversation_finish_turn", {
      request: { assistantMessageId, content, status },
    });
  }
}
