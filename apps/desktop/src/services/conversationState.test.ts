import { describe, expect, it } from "vitest";

import {
  createConversationState,
  selectVisibleConversations,
  workspaceReducer,
} from "./conversationState";
import type {
  ConversationBootstrap,
  ConversationDetail,
  StartedTurn,
} from "./conversationService";

const conversationA: ConversationDetail = {
  id: "a",
  title: "Rust bridge",
  createdAt: 1,
  updatedAt: 2,
  messages: [],
};

const conversationB: ConversationDetail = {
  id: "b",
  title: "모델 비교",
  createdAt: 1,
  updatedAt: 1,
  messages: [],
};

const bootstrap: ConversationBootstrap = {
  conversations: [conversationA, conversationB],
  selected: conversationA,
};

const turn: StartedTurn = {
  conversation: { ...conversationA, updatedAt: 3 },
  user: {
    id: "user-1",
    conversationId: "a",
    role: "user",
    content: "question",
    status: "complete",
    createdAt: 3,
    updatedAt: 3,
  },
  assistant: {
    id: "assistant-1",
    conversationId: "a",
    role: "assistant",
    content: "",
    status: "streaming",
    createdAt: 3,
    updatedAt: 3,
  },
};

function bootstrappedState() {
  return workspaceReducer(createConversationState(), { type: "bootstrapped", value: bootstrap });
}

describe("conversationState", () => {
  it("routes tokens to the bound conversation after selection changes", () => {
    let state = workspaceReducer(bootstrappedState(), { type: "turn-started", value: turn });
    state = workspaceReducer(state, { type: "request-bound", requestHandle: "42" });
    state = workspaceReducer(state, { type: "selected", detail: conversationB });
    state = workspaceReducer(state, { type: "token", requestHandle: "42", text: "answer" });

    expect(state.details.a.messages[state.details.a.messages.length - 1]?.content).toBe("answer");
    expect(state.details.b.messages).toEqual([]);
    expect(state.selectedConversationId).toBe("b");
  });

  it("adopts an event handle before submit resolves and finalizes the source", () => {
    let state = workspaceReducer(bootstrappedState(), { type: "turn-started", value: turn });
    state = workspaceReducer(state, { type: "token", requestHandle: "9", text: "partial" });
    state = workspaceReducer(state, {
      type: "terminal",
      requestHandle: "9",
      status: "cancelled",
      tail: " tail",
    });

    expect(state.details.a.messages[state.details.a.messages.length - 1]).toMatchObject({
      content: "partial tail",
      status: "cancelled",
    });
    expect(state.activeTurn).toBeNull();
  });

  it("filters normalized titles without mutating persisted order", () => {
    const original = bootstrappedState();
    const state = workspaceReducer(original, { type: "search", value: "  RUST " });

    expect(selectVisibleConversations(state).map((item) => item.title)).toEqual(["Rust bridge"]);
    expect(state.conversations).toEqual(original.conversations);
  });
});
