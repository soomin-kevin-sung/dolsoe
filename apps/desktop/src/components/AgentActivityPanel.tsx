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
import { useEffect, useState } from "react";

import { agentCopy, type AgentCopyState } from "../services/agentCopy";
import type { AgentRunTrace, AgentToolTrace } from "../services/conversationService";

interface AgentActivityPanelProps {
  run: AgentRunTrace;
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

function toolOutput(tool: AgentToolTrace): string {
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

export function AgentActivityPanel({ run }: AgentActivityPanelProps) {
  const [expanded, setExpanded] = useState(
    run.status === "running" || run.status === "prepared",
  );
  const hasTools = run.tools.length > 0;
  const running = run.status === "running" || run.status === "prepared";
  const state = copyState(run);
  const activeTool = run.tools.find((tool) => tool.status === "running" || tool.status === "prepared");
  const completedToolCount = run.tools.filter((tool) => tool.status === "complete").length;
  const activePresentation = activeTool ? toolPresentation(activeTool.toolName) : null;

  useEffect(() => {
    if (running || run.status === "error") setExpanded(true);
    else if (run.status === "complete") setExpanded(false);
  }, [run.status, running]);

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
      className={`agent-activity is-${run.status} ${running ? "is-running" : ""}`}
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
      {hasTools && expanded && (
        <div className="agent-tool-list">
          {run.tools.map((tool) => {
            const { Icon, label } = toolPresentation(tool.toolName);
            return (
              <div className={`agent-tool-row is-${tool.status}`} key={tool.activityId}>
                <span className="agent-tool-node" aria-hidden="true">
                  <ToolStatusIcon status={tool.status} />
                </span>
                <div className="agent-tool-copy">
                  <div className="agent-tool-heading">
                    <span><Icon size={13} />{label}</span>
                    {tool.status !== "running" && tool.status !== "prepared" && (
                      <span className="agent-tool-duration">{durationLabel(tool.durationMs)}</span>
                    )}
                  </div>
                  <code className="agent-tool-input">{tool.input}</code>
                  {tool.status !== "running" && tool.status !== "prepared" && (
                    <div className="agent-tool-result">
                      <span>{toolOutput(tool) || "결과 없음"}</span>
                    </div>
                  )}
                </div>
              </div>
            );
          })}
        </div>
      )}
    </section>
  );
}
