import { invoke } from "@tauri-apps/api/core";

export interface PersonaPromptDocument {
  path: string;
  label: string;
  description: string;
  content: string;
  characterCount: number;
}

export interface PersonaPromptDraft {
  enabled: boolean;
  documents: Array<{ path: string; content: string }>;
}

export interface PersonaPromptState {
  id: string;
  name: string;
  version: number;
  enabled: boolean;
  revision: string;
  compiledPrompt: string;
  characterCount: number;
  estimatedTokens: number;
  directoryPath: string;
  documents: PersonaPromptDocument[];
}

export interface PromptPreviewMessage {
  role: "system" | "user" | "assistant";
  content: string;
}

export interface ConversationPromptPreview {
  personaId: string;
  revision: string;
  source: "conversation-snapshot" | "active-persona";
  messages: PromptPreviewMessage[];
  formattedPrompt: string;
  characterCount: number;
  estimatedTokens: number;
}

export interface PersonaPromptBindings {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
}

const tauriBindings: PersonaPromptBindings = { invoke };

export class PersonaPromptService {
  constructor(private readonly bindings: PersonaPromptBindings = tauriBindings) {}

  getState(): Promise<PersonaPromptState> {
    return this.bindings.invoke("persona_get_state");
  }

  preview(request: PersonaPromptDraft): Promise<PersonaPromptState> {
    return this.bindings.invoke("persona_preview", { request });
  }

  save(request: PersonaPromptDraft): Promise<PersonaPromptState> {
    return this.bindings.invoke("persona_save", { request });
  }

  resetDefaults(): Promise<PersonaPromptState> {
    return this.bindings.invoke("persona_reset_defaults");
  }

  previewConversation(conversationId: string): Promise<ConversationPromptPreview> {
    return this.bindings.invoke("persona_preview_conversation", { conversationId });
  }
}

export function promptDraftFromState(state: PersonaPromptState): PersonaPromptDraft {
  return {
    enabled: state.enabled,
    documents: state.documents.map((document) => ({
      path: document.path,
      content: document.content,
    })),
  };
}

export function samePromptDraft(left: PersonaPromptDraft, right: PersonaPromptDraft): boolean {
  return left.enabled === right.enabled
    && left.documents.length === right.documents.length
    && left.documents.every((document, index) => (
      document.path === right.documents[index]?.path
      && document.content === right.documents[index]?.content
    ));
}
