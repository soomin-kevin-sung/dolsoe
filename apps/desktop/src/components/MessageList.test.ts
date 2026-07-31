import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import type { Message } from "../services/runtime";
import {
  getLatestUserMessageId,
  isCpuRuntimeRecoveryError,
  isNearScrollBottom,
  MessageList,
  shouldShowAgentActivity,
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

  it("appears while the first ReAct decision is pending", () => {
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "",
      agentRun: { ...agentRun, phase: "thinking" },
    })).toBe(true);
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "",
      agentRun: { ...agentRun, phase: "choosing-tool" },
    })).toBe(true);
  });

  it("stays hidden after a tool activity card appears", () => {
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "",
      agentRun: {
        ...agentRun,
        phase: "choosing-tool",
        tools: [{
          activityId: "tool-1",
          toolName: "list_files",
          status: "running",
          input: ".",
          output: "",
          durationMs: 0,
        }],
      },
    })).toBe(false);
  });

  it("appears while user-facing answer text is streaming", () => {
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "",
    })).toBe(true);
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "답변을 작성",
    })).toBe(true);
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "답변을 작성",
      agentRun: { ...agentRun, phase: "writing" },
    })).toBe(true);
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "",
      agentRun: { ...agentRun, phase: "writing" },
    })).toBe(true);
  });

  it("waits for answer text before showing the cursor again after tool activity", () => {
    const toolRun = {
      ...agentRun,
      phase: "writing" as const,
      tools: [{
        activityId: "tool-1",
        toolName: "list_files",
        status: "complete" as const,
        input: ".",
        output: "README.md",
        durationMs: 12,
      }],
    };

    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "",
      agentRun: toolRun,
    })).toBe(false);
    expect(shouldShowStreamingCursor({
      status: "streaming",
      content: "파일이 있습니다.",
      agentRun: toolRun,
    })).toBe(true);
  });
});

describe("agent activity visibility", () => {
  const agentRun = {
    runId: "run-1",
    assistantMessageId: "assistant-1",
    mode: "react" as const,
    status: "complete" as const,
    startedAt: 1,
    finishedAt: 2,
    phase: "writing" as const,
    tools: [],
  };

  it("hides answer-only ReAct runs", () => {
    expect(shouldShowAgentActivity({ agentRun })).toBe(false);
    const markup = renderToStaticMarkup(createElement(MessageList, {
      state: "ready",
      messages: [{
        id: "assistant-1",
        role: "assistant",
        content: "도구 없이 바로 답합니다.",
        status: "complete",
        agentRun,
      }],
    }));

    expect(markup).not.toContain("agent-activity");
  });

  it("shows ReAct runs that used a tool", () => {
    const toolRun = {
      ...agentRun,
      tools: [{
        activityId: "tool-1",
        toolName: "calculator",
        status: "complete" as const,
        input: "{\"expression\":\"2 + 2\"}",
        output: "4",
        durationMs: 1,
      }],
    };
    expect(shouldShowAgentActivity({ agentRun: toolRun })).toBe(true);

    const markup = renderToStaticMarkup(createElement(MessageList, {
      state: "ready",
      messages: [{
        id: "assistant-1",
        role: "assistant",
        content: "결과는 4입니다.",
        status: "complete",
        agentRun: toolRun,
      }],
    }));

    expect(markup).toContain("agent-activity");
    expect(markup).toContain("assistant-content has-agent-activity");
    expect(markup).toContain("agent-activity is-complete is-collapsed");
    expect(markup.indexOf('<div class="message-author">돌쇠</div>'))
      .toBeLessThan(markup.indexOf('<section class="agent-activity'));
    expect(markup.indexOf('<section class="agent-activity'))
      .toBeLessThan(markup.indexOf('<div class="message-text">결과는 4입니다.</div>'));
  });
});
