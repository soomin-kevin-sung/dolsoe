import { describe, expect, it } from "vitest";

import { ConversationService, type ConversationBindings } from "./conversationService";

describe("ConversationService", () => {
  it("uses fixed Tauri command names and camelCase arguments", async () => {
    const calls: Array<[string, Record<string, unknown> | undefined]> = [];
    const bindings: ConversationBindings = {
      invoke: async <T>(command: string, args?: Record<string, unknown>) => {
        calls.push([command, args]);
        return true as T;
      },
    };
    const service = new ConversationService(bindings);

    await service.load("conversation-1");
    await service.rename("conversation-1", "새 이름");
    await service.startNewTurn("첫 질문", "react");
    await service.startTurn("conversation-1", "질문");
    await service.getAgentPreferences();
    await service.setDefaultAgentMode("react");
    await service.setConversationAgentMode("conversation-1", "react");
    await service.finishTurn("assistant-1", "답변", "cancelled");

    expect(calls).toEqual([
      ["conversation_load", { conversationId: "conversation-1" }],
      ["conversation_rename", { conversationId: "conversation-1", title: "새 이름" }],
      ["conversation_start_new_turn", { prompt: "첫 질문", agentMode: "react" }],
      ["conversation_start_turn", { conversationId: "conversation-1", prompt: "질문" }],
      ["agent_get_preferences", undefined],
      ["agent_set_default_mode", { mode: "react" }],
      ["conversation_set_agent_mode", { conversationId: "conversation-1", mode: "react" }],
      ["conversation_finish_turn", { request: { assistantMessageId: "assistant-1", content: "답변", status: "cancelled" } }],
    ]);
  });
});
