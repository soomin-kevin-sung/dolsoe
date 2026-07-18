import { Activity, LoaderCircle, Search, SquarePen } from "lucide-react";
import type { Ref } from "react";
import type { Session } from "../services/runtime";
import { IconButton } from "./IconButton";

interface SidebarProps { sessions: Session[]; diagnosticsOpen: boolean; searchInputRef: Ref<HTMLInputElement>; onNew(): void; onDiagnostics(): void; onSelect(sessionId: string): void; }

export function Sidebar({ sessions, diagnosticsOpen, searchInputRef, onNew, onDiagnostics, onSelect }: SidebarProps) {
  return (
    <nav className="sidebar" aria-label="대화 목록">
      <div className="sidebar-header">
        <svg className="app-mark" viewBox="0 0 48 48" aria-hidden="true"><rect x="9" y="9" width="30" height="30" rx="5" fill="var(--accent)"/><path d="M24 18.5c-2.2-1.6-4.8-2.1-7-2.1v12.4c2.2 0 4.8.5 7 2.1 2.2-1.6 4.8-2.1 7-2.1V16.4c-2.2 0-4.8.5-7 2.1z" fill="var(--bg-surface)"/><rect x="3" y="15" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="3" y="27" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="41" y="15" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="41" y="27" width="4" height="6" rx="2" fill="var(--accent)"/></svg>
        <span className="app-name">Local LLM Wiki</span>
        <IconButton icon={SquarePen} label="새 대화" onClick={onNew} />
      </div>
      <label className="search-wrap">
        <Search size={14} strokeWidth={2} aria-hidden="true" />
        <input ref={searchInputRef} className="search-input" type="search" aria-label="대화 검색" placeholder="대화 검색" />
      </label>
      <div className="session-list">
        {sessions.map((session) => (
          <button key={session.id} type="button" className={`session-item ${session.active ? "active" : ""}`} onClick={() => onSelect(session.id)}>
            <div className="session-title">{session.title}</div>
            <div className="session-meta">
              {session.generating && <span className="generating"><LoaderCircle className="spin" size={14} />생성 중</span>}
              {session.queued && <span className="queued">대기 중</span>}
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
