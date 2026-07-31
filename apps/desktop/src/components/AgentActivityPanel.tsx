import {
  Calculator,
  Check,
  ChevronDown,
  FileSearch,
  FileText,
  FolderOpen,
  Info,
  LoaderCircle,
  Square,
  TriangleAlert,
  Wrench,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { agentCopy, type AgentCopyState } from "../services/agentCopy";
import type {
  AgentRunTrace,
  AgentToolTrace,
  MessageStatus,
} from "../services/conversationService";

interface AgentActivityPanelProps {
  run: AgentRunTrace;
  messageStatus?: MessageStatus;
}

function copyState(run: AgentRunTrace): AgentCopyState {
  if (run.status === "complete") return "completed";
  if (run.status === "cancelled" || run.status === "interrupted") return "cancelled";
  if (run.status === "error") return "failed";
  if (run.tools.some((tool) => tool.status === "running" && tool.toolName === "calculator")) {
    return "calculator";
  }
  if (run.tools.some((tool) => tool.status === "running")) return "files";
  return run.phase ?? "thinking";
}

function jsonRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null && !Array.isArray(value)
    ? value as Record<string, unknown>
    : null;
}

function parseToolJson(output: string): Record<string, unknown> | null {
  try {
    return jsonRecord(JSON.parse(output));
  } catch {
    return null;
  }
}

function formatListFilesOutput(result: Record<string, unknown>): string | null {
  if (!Array.isArray(result.entries)) return null;
  const entries = result.entries.flatMap((value) => {
    const entry = jsonRecord(value);
    if (!entry || typeof entry.name !== "string") return [];
    return `${entry.name}${entry.type === "directory" ? "/" : ""}`;
  });
  const omitted = [result.omittedHiddenOrSystem, result.omittedExternalLinks]
    .reduce<number>((total, value) => total + (typeof value === "number" ? value : 0), 0);
  const notes = [
    result.truncated === true ? "목록 일부만 표시" : "",
    omitted > 0 ? `숨김 또는 외부 항목 ${omitted}개 제외` : "",
  ].filter(Boolean);

  return [
    entries.join("\n") || "항목 없음",
    notes.length > 0 ? `\n${notes.join(" · ")}` : "",
  ].join("");
}

function formatSearchFilesOutput(result: Record<string, unknown>): string | null {
  if (!Array.isArray(result.matches)) return null;
  const matches = result.matches.flatMap((value) => {
    const match = jsonRecord(value);
    if (!match || typeof match.path !== "string") return [];
    const line = typeof match.line === "number" ? `:${match.line}` : "";
    const preview = typeof match.preview === "string" ? `  ${match.preview}` : "";
    return `${match.path}${line}${preview}`;
  });
  return matches.join("\n") || "검색 결과 없음";
}

export function formatToolOutput(tool: AgentToolTrace): string {
  const parsed = parseToolJson(tool.output);
  if (parsed && tool.toolName === "list_files") {
    return formatListFilesOutput(parsed) ?? tool.output;
  }
  if (parsed && tool.toolName === "read_file" && typeof parsed.content === "string") {
    return parsed.content || "빈 파일";
  }
  if (parsed && tool.toolName === "search_files") {
    return formatSearchFilesOutput(parsed) ?? tool.output;
  }
  if (parsed) return JSON.stringify(parsed, null, 2);
  return tool.output
    .replace(/^Calculator result:\s*/, "")
    .replace(/^Calculator error:\s*/, "");
}

function durationLabel(durationMs: number): string {
  if (durationMs < 1) return "< 1ms";
  if (durationMs < 1_000) return `${durationMs}ms`;
  return `${(durationMs / 1_000).toFixed(1)}s`;
}

function ToolStatusIcon({ status }: Pick<AgentToolTrace, "status">) {
  if (status === "running" || status === "prepared") {
    return <LoaderCircle className="spin" size={12} />;
  }
  if (status === "complete") return <Check size={14} />;
  if (status === "cancelled" || status === "interrupted") {
    return <Square size={10} fill="currentColor" />;
  }
  return <TriangleAlert size={14} />;
}

function toolPresentation(toolName: string) {
  switch (toolName) {
    case "calculator":
      return { Icon: Calculator, label: "계산기" };
    case "list_files":
      return { Icon: FolderOpen, label: "폴더 살펴보기" };
    case "read_file":
      return { Icon: FileText, label: "파일 읽기" };
    case "search_files":
      return { Icon: FileSearch, label: "파일 검색" };
    case "get_file_info":
      return { Icon: Info, label: "파일 정보" };
    default:
      return { Icon: Wrench, label: toolName };
  }
}

export function toolResultMeta(tool: AgentToolTrace): string {
  if (tool.status === "error") return "실행 실패";
  const parsed = parseToolJson(tool.output);
  if (!parsed) return "";

  if (tool.toolName === "list_files" && Array.isArray(parsed.entries)) {
    const omitted = [parsed.omittedHiddenOrSystem, parsed.omittedExternalLinks]
      .reduce<number>((total, value) => total + (typeof value === "number" ? value : 0), 0);
    return [
      `${parsed.entries.length}개 항목`,
      omitted > 0 ? `${omitted}개 제외` : "",
    ].filter(Boolean).join(" · ");
  }
  if (tool.toolName === "search_files" && Array.isArray(parsed.matches)) {
    return `${parsed.matches.length}개 일치`;
  }
  if (tool.toolName === "read_file" && typeof parsed.returnedLines === "number") {
    return `${parsed.returnedLines}줄`;
  }
  if (tool.toolName === "get_file_info" && typeof parsed.type === "string") {
    return parsed.type;
  }
  return "";
}

function ToolResult({ tool }: { tool: AgentToolTrace }) {
  const output = formatToolOutput(tool).trim() || "결과 없음";
  const meta = toolResultMeta(tool);
  const failed = tool.status === "error";

  return (
    <div className="agent-tool-result">
      <div className="agent-tool-result-heading">
        <span>{failed ? "오류" : "결과"}</span>
        {meta && <span>{meta}</span>}
      </div>
      <pre className="agent-tool-result-value">{output}</pre>
    </div>
  );
}

function isToolTerminal(tool: AgentToolTrace): boolean {
  return tool.status !== "running" && tool.status !== "prepared";
}

export function shouldCollapseCompletedActivity(
  runStatus: AgentRunTrace["status"],
  messageStatus?: MessageStatus,
): boolean {
  return runStatus === "complete" && messageStatus === "complete";
}

function AgentToolRow({
  tool,
  defaultExpanded,
}: {
  tool: AgentToolTrace;
  defaultExpanded: boolean;
}) {
  const terminal = isToolTerminal(tool);
  const [expanded, setExpanded] = useState(defaultExpanded && terminal);
  const previousStatusRef = useRef(tool.status);
  const { Icon, label } = toolPresentation(tool.toolName);

  useEffect(() => {
    const previous = previousStatusRef.current;
    if (
      (previous === "running" || previous === "prepared")
      && terminal
    ) {
      setExpanded(true);
    }
    previousStatusRef.current = tool.status;
  }, [terminal, tool.status]);

  const rowContent = (
    <>
      <span className="agent-tool-kind" aria-hidden="true">
        <Icon size={13} />
      </span>
      <span className="agent-tool-copy">
        <strong>{label}</strong>
        <code className="agent-tool-input">{tool.input}</code>
      </span>
      <span className={`agent-tool-duration ${terminal ? "" : "is-live"}`}>
        {terminal ? durationLabel(tool.durationMs) : "실행 중"}
      </span>
      {terminal ? (
        <ChevronDown className={expanded ? "is-open" : ""} size={14} aria-hidden="true" />
      ) : (
        <span aria-hidden="true" />
      )}
    </>
  );

  return (
    <div className={`agent-tool-row is-${tool.status}`}>
      <span className="agent-tool-node" aria-hidden="true">
        <ToolStatusIcon status={tool.status} />
      </span>
      <div className={`agent-tool-surface ${expanded ? "is-expanded" : ""}`}>
        {terminal ? (
          <button
            className="agent-tool-toggle"
            type="button"
            aria-expanded={expanded}
            onClick={() => setExpanded((value) => !value)}
          >
            {rowContent}
          </button>
        ) : (
          <div className="agent-tool-toggle">{rowContent}</div>
        )}
        {terminal && (
          <div
            className={`agent-tool-result-shell ${expanded ? "is-expanded" : "is-collapsed"}`}
            aria-hidden={!expanded}
            inert={!expanded}
          >
            <ToolResult tool={tool} />
          </div>
        )}
      </div>
    </div>
  );
}

export function AgentActivityPanel({ run, messageStatus }: AgentActivityPanelProps) {
  const [expanded, setExpanded] = useState(
    !shouldCollapseCompletedActivity(run.status, messageStatus),
  );
  const hasTools = run.tools.length > 0;
  const running = run.status === "running" || run.status === "prepared";
  const state = copyState(run);
  const activeTool = run.tools.find((tool) => tool.status === "running" || tool.status === "prepared");
  const completedToolCount = run.tools.filter((tool) => tool.status === "complete").length;
  const activePresentation = activeTool ? toolPresentation(activeTool.toolName) : null;

  useEffect(() => {
    if (running || run.status === "error") setExpanded(true);
    else if (shouldCollapseCompletedActivity(run.status, messageStatus)) setExpanded(false);
  }, [messageStatus, run.status, running]);

  const label = agentCopy(run.runId, state);
  const statusIcon = running
    ? <LoaderCircle className="spin" size={15} />
    : run.status === "complete"
      ? <Check size={15} />
        : run.status === "cancelled" || run.status === "interrupted"
          ? <Square size={12} fill="currentColor" />
          : <TriangleAlert size={15} />;
  const CurrentIcon = activePresentation?.Icon;
  const detail = activePresentation
    ? `${activePresentation.label} · ${completedToolCount}개 마침`
    : run.phase === "writing"
      ? "답변을 정리하고 있습니다"
      : run.status === "complete"
        ? `도구 ${run.tools.length}회 사용`
        : "요청을 살펴보고 있습니다";
  const activityHeader = (
    <>
      <span className="agent-activity-summary">
        <span className="agent-activity-current" aria-hidden="true">
          {CurrentIcon ? <CurrentIcon size={15} /> : statusIcon}
        </span>
        <span className="agent-activity-copy">
          <span className="agent-activity-label">{label}</span>
          <small>{detail}</small>
        </span>
      </span>
      <span className="agent-activity-controls" aria-hidden="true">
        {running && (
          <span className="agent-activity-meter">
            <i />
            <i />
            <i />
          </span>
        )}
        {hasTools && <ChevronDown className={expanded ? "is-open" : ""} size={15} />}
      </span>
    </>
  );

  return (
    <section
      className={[
        "agent-activity",
        `is-${run.status}`,
        running && run.status !== "running" ? "is-running" : "",
        expanded ? "is-expanded" : "is-collapsed",
      ].filter(Boolean).join(" ")}
      aria-live="polite"
    >
      {hasTools ? (
        <button
          className="agent-activity-toggle"
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          {activityHeader}
        </button>
      ) : (
        <div className="agent-activity-toggle">{activityHeader}</div>
      )}
      {hasTools && (
        <div
          className={`agent-tool-list-shell ${expanded ? "is-expanded" : "is-collapsed"}`}
          aria-hidden={!expanded}
          inert={!expanded}
        >
          <div className={`agent-tool-list ${run.tools.length === 1 ? "is-single-tool" : ""}`}>
            {run.tools.map((tool, index) => (
              <AgentToolRow
                defaultExpanded={index === 0}
                key={tool.activityId}
                tool={tool}
              />
            ))}
          </div>
        </div>
      )}
    </section>
  );
}
