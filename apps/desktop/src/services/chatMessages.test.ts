import { describe, expect, it } from "vitest";

import type { StoredMessage } from "./conversationService";
import { chatMessagesForPrompt } from "./chatMessages";

function message(role: "user" | "assistant", content: string, status: StoredMessage["status"]): StoredMessage {
  return {
    id: `${role}-${content}`,
    conversationId: "conversation",
    role,
    content,
    status,
    kind: "chat",
    source: role === "user" ? "user" : "model",
    metadataJson: null,
    createdAt: 1,
    updatedAt: 1,
  };
}

describe("chatMessagesForPrompt", () => {
  it("includes completed turns and appends the new user prompt", () => {
    expect(chatMessagesForPrompt([
      message("user", "내 이름은 수민이야", "complete"),
      message("assistant", "반가워요, 수민님.", "complete"),
    ], "내 이름이 뭐야?")).toEqual([
      { role: "user", content: "내 이름은 수민이야" },
      { role: "assistant", content: "반가워요, 수민님." },
      { role: "user", content: "내 이름이 뭐야?" },
    ]);
  });

  it("excludes cancelled and incomplete turns", () => {
    expect(chatMessagesForPrompt([
      message("user", "안녕", "complete"),
      message("assistant", "불완전한 답", "cancelled"),
    ], "다시 인사해줘")).toEqual([
      { role: "user", content: "다시 인사해줘" },
    ]);
  });
});
