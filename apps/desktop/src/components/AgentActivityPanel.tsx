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
  if (status === "running") return <LoaderCircle className="spin" size={14} />;
  if (status === "complete") return <Check size={14} />;
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
  const [expanded, setExpanded] = useState(run.status === "running");
  const hasTools = run.tools.length > 0;
  const running = run.status === "running" || run.status === "prepared";
  const state = copyState(run);

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

  return (
    <section className={`agent-activity ${running ? "is-running" : ""}`} aria-live="polite">
      {hasTools ? (
        <button
          className="agent-activity-toggle"
          type="button"
          aria-expanded={expanded}
          onClick={() => setExpanded((value) => !value)}
        >
          <span className="agent-activity-state">
            {statusIcon}
            <span className="agent-activity-label">{label}</span>
          </span>
          <ChevronDown className={expanded ? "is-open" : ""} size={15} />
        </button>
      ) : (
        <div className="agent-activity-toggle">
          <span className="agent-activity-state">
            {statusIcon}
            <span className="agent-activity-label">{label}</span>
          </span>
        </div>
      )}
      {hasTools && expanded && (
        <div className="agent-tool-list">
          {run.tools.map((tool) => {
            const { Icon, label } = toolPresentation(tool.toolName);
            return (
              <div className={`agent-tool-row is-${tool.status}`} key={tool.activityId}>
                <Icon size={15} />
                <div className="agent-tool-copy">
                  <div className="agent-tool-heading">
                    <span>{label}</span>
                    <code>{tool.input}</code>
                  </div>
                  {tool.status !== "running" && (
                    <div className="agent-tool-result">
                      <ToolStatusIcon status={tool.status} />
                      <span>{toolOutput(tool) || "결과 없음"}</span>
                      <span className="agent-tool-duration">{durationLabel(tool.durationMs)}</span>
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
