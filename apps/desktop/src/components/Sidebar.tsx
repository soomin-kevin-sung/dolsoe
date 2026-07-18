import { Activity, LoaderCircle, Search, SquarePen } from "lucide-react";
import type { Session } from "../services/runtime";
import { IconButton } from "./IconButton";

interface SidebarProps { sessions: Session[]; diagnosticsOpen: boolean; onNew(): void; onDiagnostics(): void; }

export function Sidebar({ sessions, diagnosticsOpen, onNew, onDiagnostics }: SidebarProps) {
  return (
    <nav className="sidebar" aria-label="대화 목록">
      <div className="sidebar-header">
        <span className="app-mark" aria-hidden="true">L</span>
        <span className="app-name">Local LLM Wiki</span>
        <IconButton icon={SquarePen} label="새 대화" onClick={onNew} />
      </div>
      <label className="search-wrap">
        <Search size={14} strokeWidth={2} aria-hidden="true" />
        <input className="search-input" type="search" aria-label="대화 검색" placeholder="대화 검색" />
      </label>
      <div className="session-list">
        {sessions.map((session) => (
          <button key={session.id} type="button" className={`session-item ${session.active ? "active" : ""}`}>
            <div className="session-title">{session.title}</div>
            <div className="session-meta">
              {session.generating && <span className="generating"><LoaderCircle className="spin" size={14} />생성 중</span>}
              <span>{session.meta}</span>
            </div>
          </button>
        ))}
      </div>
      <div className="sidebar-footer">
        <button type="button" className={`footer-button ${diagnosticsOpen ? "active" : ""}`} onClick={onDiagnostics}>
          <Activity size={16} aria-hidden="true" />진단
        </button>
      </div>
    </nav>
  );
}
