import { describe, expect, it } from "vitest";

import { AGENT_MODES, DEFAULT_AGENT_MODE, getAgentMode } from "./agentModes";

describe("agent mode catalog", () => {
  it("ships Chat and ReAct while keeping Chat as the default", () => {
    expect(DEFAULT_AGENT_MODE).toBe("chat");
    expect(AGENT_MODES.filter((mode) => mode.availability === "available").map((mode) => mode.id)).toEqual(["chat", "react"]);
  });

  it("keeps Plan & Solve as coming soon", () => {
    expect(getAgentMode("react").availability).toBe("available");
    expect(getAgentMode("plan-and-solve").availability).toBe("coming-soon");
  });
});
