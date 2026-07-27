import { describe, expect, it } from "vitest";

import { AGENT_MODES, DEFAULT_AGENT_MODE, getAgentMode } from "./agentModes";

describe("agent mode catalog", () => {
  it("keeps Chat as the only available mode", () => {
    expect(DEFAULT_AGENT_MODE).toBe("chat");
    expect(AGENT_MODES.filter((mode) => mode.availability === "available").map((mode) => mode.id)).toEqual(["chat"]);
  });

  it("exposes future modes as coming soon", () => {
    expect(getAgentMode("react").availability).toBe("coming-soon");
    expect(getAgentMode("plan-and-solve").availability).toBe("coming-soon");
  });
});
