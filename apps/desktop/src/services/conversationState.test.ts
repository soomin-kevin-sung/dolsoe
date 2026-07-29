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
  agentMode: "chat",
  workspacePath: "C:\\workspace-a",
  createdAt: 1,
  updatedAt: 2,
  messages: [],
  agentRuns: [],
};

const conversationB: ConversationDetail = {
  id: "b",
  title: "모델 비교",
  agentMode: "chat",
  workspacePath: "C:\\workspace-b",
  createdAt: 1,
  updatedAt: 1,
  messages: [],
  agentRuns: [],
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
    kind: "chat",
    source: "user",
    metadataJson: null,
    createdAt: 3,
    updatedAt: 3,
  },
  assistant: {
    id: "assistant-1",
    conversationId: "a",
    role: "assistant",
    content: "",
    status: "streaming",
    kind: "chat",
    source: "model",
    metadataJson: null,
    createdAt: 3,
    updatedAt: 3,
  },
};

function bootstrappedState() {
  return workspaceReducer(createConversationState(), { type: "bootstrapped", value: bootstrap });
}

describe("conversationState", () => {
  it("supports an empty bootstrap and promotes a draft on the first turn", () => {
    let state = workspaceReducer(createConversationState(), {
      type: "bootstrapped",
      value: { conversations: [], selected: null },
    });
    state = workspaceReducer(state, { type: "draft-opened" });
    state = workspaceReducer(state, { type: "turn-started", value: turn });

    expect(state.selectedConversationId).toBe("a");
    expect(state.conversations).toHaveLength(1);
    expect(state.details.a.messages).toEqual([turn.user, turn.assistant]);
  });

  it("leaves no selected conversation after deleting the last persisted chat", () => {
    const initial = workspaceReducer(createConversationState(), {
      type: "bootstrapped",
      value: { conversations: [conversationA], selected: conversationA },
    });
    const state = workspaceReducer(initial, {
      type: "deleted",
      deletedId: "a",
      fallback: null,
    });

    expect(state.selectedConversationId).toBeNull();
    expect(state.conversations).toEqual([]);
  });

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

  it("marks the persisted assistant as error when native submit fails", () => {
    let state = workspaceReducer(bootstrappedState(), { type: "turn-started", value: turn });

    state = workspaceReducer(state, { type: "turn-failed", error: "runtime is busy" });

    expect(state.details.a.messages[state.details.a.messages.length - 1]).toMatchObject({
      content: "runtime is busy",
      status: "error",
    });
    expect(state.activeTurn).toBeNull();
  });

  it("tracks ReAct activity and resets a repaired public answer", () => {
    const reactTurn: StartedTurn = {
      ...turn,
      conversation: { ...turn.conversation, agentMode: "react" },
      agentRunId: "run-react",
      agentStepId: "step-react",
    };
    let state = workspaceReducer(bootstrappedState(), {
      type: "turn-started",
      value: reactTurn,
    });
    state = workspaceReducer(state, {
      type: "agent-activity",
      value: {
        kind: "tool-started",
        runId: "run-react",
        conversationId: "a",
        assistantMessageId: "assistant-1",
        activityId: "tool-1",
        toolName: "calculator",
        input: "2 + 2",
      },
    });
    state = workspaceReducer(state, {
      type: "agent-activity",
      value: {
        kind: "tool-completed",
        runId: "run-react",
        conversationId: "a",
        assistantMessageId: "assistant-1",
        activityId: "tool-1",
        toolName: "calculator",
        output: "4",
        durationMs: 1,
      },
    });
    state = workspaceReducer(state, { type: "token", requestHandle: "8", text: "partial" });
    state = workspaceReducer(state, {
      type: "agent-activity",
      value: {
        kind: "answer-reset",
        runId: "run-react",
        conversationId: "a",
        assistantMessageId: "assistant-1",
      },
    });

    expect(state.details.a.agentRuns[0].tools[0]).toMatchObject({
      status: "complete",
      input: "2 + 2",
      output: "4",
      durationMs: 1,
    });
    expect(state.details.a.messages[state.details.a.messages.length - 1]?.content).toBe("");
  });
});
