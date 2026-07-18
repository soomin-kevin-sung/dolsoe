import { Activity, LoaderCircle, MoreHorizontal, Search, SquarePen } from "lucide-react";
import { useState, type KeyboardEvent, type Ref } from "react";
import type { Session } from "../services/runtime";
import { IconButton } from "./IconButton";

interface SidebarProps {
  sessions: Session[];
  diagnosticsOpen: boolean;
  searchInputRef: Ref<HTMLInputElement>;
  searchValue?: string;
  onSearchChange?(value: string): void;
  onNew(): void;
  onDiagnostics(): void;
  onSelect(sessionId: string): void;
  onRename?(sessionId: string, title: string): void | Promise<void>;
  onClear?(sessionId: string): void;
  onDelete?(sessionId: string): void;
}

export function Sidebar({
  sessions,
  diagnosticsOpen,
  searchInputRef,
  searchValue,
  onSearchChange,
  onNew,
  onDiagnostics,
  onSelect,
  onRename,
  onClear,
  onDelete,
}: SidebarProps) {
  const [localSearch, setLocalSearch] = useState("");
  const [menuId, setMenuId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useState("");
  const query = (searchValue ?? localSearch).trim().toLocaleLowerCase();
  const visible = query
    ? sessions.filter((session) => session.title.toLocaleLowerCase().includes(query))
    : sessions;

  function changeSearch(value: string) {
    setLocalSearch(value);
    onSearchChange?.(value);
  }

  function beginRename(session: Session) {
    setEditingId(session.id);
    setDraftTitle(session.title);
    setMenuId(null);
  }

  function commitRename(sessionId: string) {
    const title = draftTitle.trim();
    if (title) void onRename?.(sessionId, title);
    setEditingId(null);
  }

  function renameKeyDown(event: KeyboardEvent<HTMLInputElement>, sessionId: string) {
    if (event.key === "Enter") {
      event.preventDefault();
      commitRename(sessionId);
    } else if (event.key === "Escape") {
      event.preventDefault();
      setEditingId(null);
    }
  }

  return (
    <nav className="sidebar" aria-label="대화 목록">
      <div className="sidebar-header">
        <svg className="app-mark" viewBox="0 0 48 48" aria-hidden="true"><rect x="9" y="9" width="30" height="30" rx="5" fill="var(--accent)"/><path d="M24 18.5c-2.2-1.6-4.8-2.1-7-2.1v12.4c2.2 0 4.8.5 7 2.1 2.2-1.6 4.8-2.1 7-2.1V16.4c-2.2 0-4.8.5-7 2.1z" fill="var(--bg-surface)"/><rect x="3" y="15" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="3" y="27" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="41" y="15" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="41" y="27" width="4" height="6" rx="2" fill="var(--accent)"/></svg>
        <span className="app-name">Local LLM Wiki</span>
        <IconButton icon={SquarePen} label="새 대화" onClick={onNew} />
      </div>
      <label className="search-wrap">
        <Search size={14} strokeWidth={2} aria-hidden="true" />
        <input ref={searchInputRef} className="search-input" type="search" aria-label="대화 검색" placeholder="대화 검색" value={searchValue ?? localSearch} onChange={(event) => changeSearch(event.target.value)} />
      </label>
      <div className="session-list">
        {visible.map((session) => (
          <div className="session-row" key={session.id}>
            <button type="button" className={`session-item ${session.active ? "active" : ""}`} onClick={() => onSelect(session.id)}>
              {editingId === session.id ? (
                <input
                  autoFocus
                  className="session-rename-input"
                  aria-label="대화 이름"
                  value={draftTitle}
                  onClick={(event) => event.stopPropagation()}
                  onChange={(event) => setDraftTitle(event.target.value)}
                  onKeyDown={(event) => renameKeyDown(event, session.id)}
                  onBlur={() => commitRename(session.id)}
                />
              ) : <div className="session-title">{session.title}</div>}
              <div className="session-meta">
                {session.generating && <span className="generating"><LoaderCircle className="spin" size={14} />생성 중</span>}
                {session.queued && <span className="queued">대기 중</span>}
                <span>{session.meta}</span>
              </div>
            </button>
            <button
              type="button"
              className="session-menu-button"
              aria-label={`${session.title} 대화 메뉴`}
              aria-expanded={menuId === session.id}
              onClick={() => setMenuId((current) => current === session.id ? null : session.id)}
            >
              <MoreHorizontal size={15} aria-hidden="true" />
            </button>
            {menuId === session.id && (
              <div className="session-actions-menu" role="menu">
                <button type="button" role="menuitem" onClick={() => beginRename(session)}>이름 변경</button>
                <button type="button" role="menuitem" onClick={() => { setMenuId(null); onClear?.(session.id); }}>대화 초기화</button>
                <button type="button" role="menuitem" className="danger" onClick={() => { setMenuId(null); onDelete?.(session.id); }}>삭제</button>
              </div>
            )}
          </div>
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
