import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import type { Message } from "../services/runtime";
import {
  getLatestUserMessageId,
  isCpuRuntimeRecoveryError,
  isNearScrollBottom,
  MessageList,
  shouldShowStreamingCursor,
  shouldShowEmptyContent,
} from "./MessageList";

describe("shouldShowEmptyContent", () => {
  it("keeps persisted messages visible when no model is loaded", () => {
    expect(shouldShowEmptyContent("no-model", 2)).toBe(false);
    expect(shouldShowEmptyContent("no-model", 0)).toBe(true);
  });
});

describe("CPU runtime recovery", () => {
  it("shows a focused install action instead of the raw native error", () => {
    const error = "CPU runtime is unavailable and the bundled recovery pack is missing";
    const markup = renderToStaticMarkup(createElement(MessageList, {
      state: "error",
      messages: [],
      error,
    }));

    expect(isCpuRuntimeRecoveryError(error)).toBe(true);
    expect(markup).toContain("런타임이 필요합니다");
    expect(markup).toContain("런타임 설치");
    expect(markup).not.toContain(error);
    expect(markup).not.toContain("모델 다시 선택");
  });
});

describe("assistant identity", () => {
  it("uses the Dolsoe name and icon for assistant messages", () => {
    const markup = renderToStaticMarkup(createElement(MessageList, {
      state: "ready",
      messages: [{ id: "assistant-1", role: "assistant", content: "안녕하세요.", status: "complete" }],
    }));

    expect(markup).toContain('<div class="message-author">돌쇠</div>');
    expect(markup).toContain('<span class="assistant-mark" aria-hidden="true"><img src="data:image/svg+xml,');
    expect(markup).not.toContain("로컬 AI");
  });

  it("uses the Dolsoe identity for an empty conversation", () => {
    const markup = renderToStaticMarkup(createElement(MessageList, {
      state: "empty",
      messages: [],
    }));

    expect(markup).toContain("empty-dolsoe-mark");
    expect(markup).toContain("돌쇠에게 일을 맡겨보세요");
    expect(markup).not.toContain("새 대화를 시작하세요");
  });
});

describe("message auto-scroll", () => {
  it("treats a small remaining distance as the bottom", () => {
    expect(isNearScrollBottom({ scrollHeight: 1_000, scrollTop: 620, clientHeight: 300 })).toBe(true);
    expect(isNearScrollBottom({ scrollHeight: 1_000, scrollTop: 500, clientHeight: 300 })).toBe(false);
  });

  it("detects a newly appended user prompt", () => {
    const messages: Message[] = [
      { id: "assistant-1", role: "assistant", content: "첫 응답" },
      { id: "user-2", role: "user", content: "다음 질문" },
      { id: "assistant-2", role: "assistant", content: "", status: "streaming" },
    ];

    expect(getLatestUserMessageId(messages)).toBe("user-2");
    expect(getLatestUserMessageId(messages.filter((message) => message.role === "assistant"))).toBeNull();
  });
});

describe("streaming cursor", () => {
  const agentRun = {
    runId: "run-1",
    assistantMessageId: "assistant-1",
    mode: "react" as const,
    status: "running" as const,
    startedAt: 1,
    finishedAt: null,
    tools: [],
  };

  it("stays hidden while an agent is working without answer text", () => {
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "",
      agentRun: { ...agentRun, phase: "thinking" },
    })).toBe(false);
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "내부 작업",
      agentRun: { ...agentRun, phase: "choosing-tool" },
    })).toBe(false);
  });

  it("appears only while user-facing answer text is streaming", () => {
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "답변을 작성",
    })).toBe(true);
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "답변을 작성",
      agentRun: { ...agentRun, phase: "writing" },
    })).toBe(true);
  });
});
