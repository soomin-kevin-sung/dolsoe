import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";

import type { AgentRunTrace } from "../services/conversationService";
import {
  AgentActivityPanel,
  formatToolOutput,
  shouldCollapseCompletedActivity,
  toolResultMeta,
} from "./AgentActivityPanel";

describe("tool result presentation", () => {
  it("collapses only after both the run and answer are complete", () => {
    expect(shouldCollapseCompletedActivity("complete", "streaming")).toBe(false);
    expect(shouldCollapseCompletedActivity("running", "complete")).toBe(false);
    expect(shouldCollapseCompletedActivity("complete", "complete")).toBe(true);
  });

  it("formats the structured list_files result as an actual directory listing", () => {
    const tool = {
      activityId: "tool-1",
      toolName: "list_files",
      status: "complete" as const,
      input: "{\"path\":\".\"}",
      output: JSON.stringify({
        path: ".",
        entries: [
          { name: "apps", type: "directory", size: null, link: false },
          { name: "Cargo.toml", type: "file", size: 42, link: false },
        ],
        omittedHiddenOrSystem: 3,
        omittedExternalLinks: 1,
        truncated: false,
      }),
      durationMs: 5,
    };
    const output = formatToolOutput(tool);

    expect(output).toContain("apps/");
    expect(output).toContain("Cargo.toml");
    expect(output).toContain("숨김 또는 외부 항목 4개 제외");
    expect(output).not.toContain("\"entries\"");
    expect(toolResultMeta(tool)).toBe("2개 항목 · 4개 제외");
  });

  it("renders the first completed tool as an expanded command row", () => {
    const output = Array.from(
      { length: 12 },
      (_, index) => `src/file-${index + 1}.ts`,
    ).join("\n");
    const run: AgentRunTrace = {
      runId: "run-1",
      assistantMessageId: "assistant-1",
      mode: "react",
      status: "running",
      startedAt: 1,
      finishedAt: null,
      phase: "choosing-tool",
      tools: [
        {
          activityId: "tool-1",
          toolName: "list_files",
          status: "complete",
          input: ".",
          output,
          durationMs: 12,
        },
      ],
    };

    const markup = renderToStaticMarkup(<AgentActivityPanel run={run} />);

    expect(markup).toContain("agent-tool-surface is-expanded");
    expect(markup).toContain("agent-tool-list is-single-tool");
    expect(markup).toContain("agent-tool-result-shell is-expanded");
    expect(markup).toContain("agent-tool-result-heading");
    expect(markup).toContain("결과");
    expect(markup).toContain("src/file-12.ts");
  });

  it("keeps later completed tool results mounted inside a collapsed shell", () => {
    const run: AgentRunTrace = {
      runId: "run-2",
      assistantMessageId: "assistant-2",
      mode: "react",
      status: "running",
      startedAt: 1,
      finishedAt: null,
      phase: "writing",
      tools: [
        {
          activityId: "tool-1",
          toolName: "calculator",
          status: "complete",
          input: "{\"expression\":\"6*7\"}",
          output: "Calculator result: 42",
          durationMs: 1,
        },
        {
          activityId: "tool-2",
          toolName: "calculator",
          status: "complete",
          input: "{\"expression\":\"7*8\"}",
          output: "Calculator result: 56",
          durationMs: 1,
        },
      ],
    };

    const markup = renderToStaticMarkup(<AgentActivityPanel run={run} />);

    expect(markup).toContain(">42</pre>");
    expect(markup).toContain(">56</pre>");
    expect(markup.match(/agent-tool-result-shell is-expanded/g)).toHaveLength(1);
    expect(markup.match(/agent-tool-result-shell is-collapsed/g)).toHaveLength(1);
  });

  it("collapses a completed run after the answer while keeping tool cards mounted", () => {
    const run: AgentRunTrace = {
      runId: "run-3",
      assistantMessageId: "assistant-3",
      mode: "react",
      status: "complete",
      startedAt: 1,
      finishedAt: 2,
      phase: "writing",
      tools: [
        {
          activityId: "tool-1",
          toolName: "calculator",
          status: "complete",
          input: "{\"expression\":\"2+2\"}",
          output: "Calculator result: 4",
          durationMs: 1,
        },
      ],
    };

    const markup = renderToStaticMarkup(
      <AgentActivityPanel messageStatus="complete" run={run} />,
    );

    expect(markup).toContain("agent-activity is-complete is-collapsed");
    expect(markup).toContain("agent-tool-list-shell is-collapsed");
    expect(markup).toContain('aria-hidden="true"');
    expect(markup).toContain("agent-tool-surface is-expanded");
  });

  it("keeps a completed tool run expanded while answer text is still streaming", () => {
    const run: AgentRunTrace = {
      runId: "run-4",
      assistantMessageId: "assistant-4",
      mode: "react",
      status: "complete",
      startedAt: 1,
      finishedAt: 2,
      phase: "writing",
      tools: [
        {
          activityId: "tool-1",
          toolName: "calculator",
          status: "complete",
          input: "{\"expression\":\"3+3\"}",
          output: "Calculator result: 6",
          durationMs: 1,
        },
      ],
    };

    const markup = renderToStaticMarkup(
      <AgentActivityPanel messageStatus="streaming" run={run} />,
    );

    expect(markup).toContain("agent-activity is-complete is-expanded");
    expect(markup).toContain("agent-tool-list-shell is-expanded");
    expect(markup).toContain('aria-hidden="false"');
  });
});
