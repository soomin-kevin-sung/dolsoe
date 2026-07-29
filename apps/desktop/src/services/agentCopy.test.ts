import { describe, expect, it } from "vitest";

import { agentCopy } from "./agentCopy";

describe("agentCopy", () => {
  it("keeps a phrase stable for the same run and state", () => {
    expect(agentCopy("run-7", "thinking")).toBe(agentCopy("run-7", "thinking"));
  });

  it("uses fixed cancellation and failure copy", () => {
    expect(agentCopy("run-a", "cancelled")).toBe("하던 일을 멈췄습니다");
    expect(agentCopy("run-b", "failed")).toBe("일을 마치지 못했습니다");
  });
});
