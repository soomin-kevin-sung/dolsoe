import { describe, expect, it } from "vitest";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { isCpuRuntimeRecoveryError, MessageList, shouldShowEmptyContent } from "./MessageList";

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
    expect(markup).toContain("CPU 런타임이 필요합니다");
    expect(markup).toContain("CPU 런타임 설치");
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
});
