import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import { AgentModeSettings } from "./AgentModeSettings";

describe("AgentModeSettings", () => {
  it("renders Chat selected and future modes disabled", () => {
    const markup = renderToStaticMarkup(createElement(AgentModeSettings));

    expect(markup).toContain("Chat");
    expect(markup).toContain("ReAct");
    expect(markup).toContain("Plan &amp; Solve");
    expect(markup.match(/준비 중/g)).toHaveLength(2);
    expect(markup.match(/disabled=""/g)).toHaveLength(2);
    expect(markup).toContain('aria-checked="true"');
  });
});
