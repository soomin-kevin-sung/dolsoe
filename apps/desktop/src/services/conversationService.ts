import { invoke } from "@tauri-apps/api/core";

export type MessageRole = "user" | "assistant";
export type MessageStatus = "complete" | "streaming" | "cancelled" | "interrupted" | "error";
export type TerminalMessageStatus = Exclude<MessageStatus, "streaming">;

export interface ConversationSummary {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
}

export interface StoredMessage {
  id: string;
  conversationId: string;
  role: MessageRole;
  content: string;
  status: MessageStatus;
  createdAt: number;
  updatedAt: number;
}

export interface ConversationDetail extends ConversationSummary {
  messages: StoredMessage[];
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

  startNewTurn(prompt: string): Promise<StartedTurn> {
    return this.bindings.invoke("conversation_start_new_turn", { prompt });
  }

  startTurn(conversationId: string, prompt: string): Promise<StartedTurn> {
    return this.bindings.invoke("conversation_start_turn", { conversationId, prompt });
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
