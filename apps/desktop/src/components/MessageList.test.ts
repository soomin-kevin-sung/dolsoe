import { describe, expect, it } from "vitest";

import { shouldShowEmptyContent } from "./MessageList";

describe("shouldShowEmptyContent", () => {
  it("keeps persisted messages visible when no model is loaded", () => {
    expect(shouldShowEmptyContent("no-model", 2)).toBe(false);
    expect(shouldShowEmptyContent("no-model", 0)).toBe(true);
  });
});
