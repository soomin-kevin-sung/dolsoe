import { describe, expect, it } from "vitest";

import {
  PersonaPromptService,
  promptDraftFromState,
  samePromptDraft,
  type PersonaPromptBindings,
  type PersonaPromptState,
} from "./personaPrompts";

const state: PersonaPromptState = {
  id: "dolsoe",
  name: "돌쇠",
  version: 1,
  enabled: true,
  revision: "abc",
  compiledPrompt: "compiled",
  characterCount: 8,
  estimatedTokens: 2,
  directoryPath: "C:\\persona",
  documents: [
    {
      path: "soul.md",
      label: "Soul",
      description: "core",
      content: "soul",
      characterCount: 4,
    },
  ],
};

describe("PersonaPromptService", () => {
  it("uses the fixed Tauri command contract", async () => {
    const calls: Array<[string, Record<string, unknown> | undefined]> = [];
    const bindings: PersonaPromptBindings = {
      invoke: async <T>(command: string, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return state as T;
      },
    };
    const service = new PersonaPromptService(bindings);
    const draft = promptDraftFromState(state);

    await service.getState();
    await service.preview(draft);
    await service.save(draft);
    await service.resetDefaults();
    await service.previewConversation("conversation-1");

    expect(calls).toEqual([
      ["persona_get_state", undefined],
      ["persona_preview", { request: draft }],
      ["persona_save", { request: draft }],
      ["persona_reset_defaults", undefined],
      ["persona_preview_conversation", { conversationId: "conversation-1" }],
    ]);
  });

  it("compares drafts by enabled state, path, and content", () => {
    const draft = promptDraftFromState(state);
    expect(samePromptDraft(draft, { ...draft, documents: [...draft.documents] })).toBe(true);
    expect(samePromptDraft(draft, {
      ...draft,
      documents: [{ ...draft.documents[0], content: "changed" }],
    })).toBe(false);
  });
});
