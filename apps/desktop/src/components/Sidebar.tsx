import { Activity, Box, ChevronRight, Cpu, Home, Layers3, LoaderCircle, MoreHorizontal, RotateCw, Search, SquarePen, TriangleAlert } from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent, type ReactNode, type Ref } from "react";
import type { Session } from "../services/runtime";
import type { HomeReadinessKind } from "../services/homeReadiness";
import { IconButton } from "./IconButton";

interface SidebarProps {
  sessions: Session[];
  homeOpen: boolean;
  diagnosticsOpen: boolean;
  readiness: HomeReadinessKind;
  runtimeLabel: string;
  modelName: string;
  modelMenuOpen: boolean;
  modelMenu: ReactNode;
  searchInputRef: Ref<HTMLInputElement>;
  searchValue?: string;
  onSearchChange?(value: string): void;
  onNew(): void;
  onHome(): void;
  onDiagnostics(): void;
  onModelMenuToggle(): void;
  onModelMenuClose(): void;
  onSelect(sessionId: string): void;
  onRename?(sessionId: string, title: string): void | Promise<void>;
  onClear?(sessionId: string): void;
  onDelete?(sessionId: string): void;
}

function runtimeTone(readiness: HomeReadinessKind): "ready" | "loading" | "pending" | "error" | "neutral" {
  if (readiness === "ready") return "ready";
  if (readiness === "runtime-installed") return "pending";
  if (readiness.includes("failed")) return "error";
  if (["runtime-checking", "runtime-downloading", "runtime-verifying", "runtime-installing", "model-loading"].includes(readiness)) return "loading";
  return "neutral";
}

function RuntimeSummaryIcon({ readiness }: { readiness: HomeReadinessKind }) {
  if (readiness === "ready") return <Layers3 size={17} strokeWidth={2} />;
  if (readiness === "runtime-installed") return <RotateCw size={16} />;
  if (readiness.includes("failed")) return <TriangleAlert size={16} />;
  if (["runtime-checking", "runtime-downloading", "runtime-verifying", "runtime-installing", "model-loading"].includes(readiness)) return <LoaderCircle className="spin" size={16} />;
  if (readiness === "model-missing") return <Box size={16} />;
  return <Cpu size={16} />;
}

function runtimeDetail(readiness: HomeReadinessKind, modelName: string): string {
  if (readiness === "ready") return modelName;
  if (readiness === "runtime-checking") return "설치 상태와 업데이트 확인 중";
  if (readiness === "runtime-downloading") return "검증된 CPU 엔진을 받는 중";
  if (readiness === "runtime-verifying") return "다운로드 파일을 확인하는 중";
  if (readiness === "runtime-installing") return "CPU 추론 엔진을 준비하는 중";
  if (readiness === "runtime-installed") return "변경 적용을 위해 재시작하세요";
  if (readiness.includes("failed")) return "설정을 열어 문제를 해결하세요";
  if (readiness === "model-loading") return modelName;
  if (readiness === "model-missing") return "사용할 GGUF 모델을 선택하세요";
  return "설정을 열어 CPU 엔진을 설치하세요";
}

export function Sidebar({
  sessions,
  homeOpen,
  diagnosticsOpen,
  readiness,
  runtimeLabel,
  modelName,
  modelMenuOpen,
  modelMenu,
  searchInputRef,
  searchValue,
  onSearchChange,
  onNew,
  onHome,
  onDiagnostics,
  onModelMenuToggle,
  onModelMenuClose,
  onSelect,
  onRename,
  onClear,
  onDelete,
}: SidebarProps) {
  const [localSearch, setLocalSearch] = useState("");
  const [menuId, setMenuId] = useState<string | null>(null);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [draftTitle, setDraftTitle] = useState("");
  const modelMenuAnchorRef = useRef<HTMLDivElement>(null);
  const modelMenuButtonRef = useRef<HTMLButtonElement>(null);
  const query = (searchValue ?? localSearch).trim().toLocaleLowerCase();
  const visible = query
    ? sessions.filter((session) => session.title.toLocaleLowerCase().includes(query))
    : sessions;

  useEffect(() => {
    if (!modelMenuOpen) return;

    const closeOutside = (event: PointerEvent) => {
      if (!modelMenuAnchorRef.current?.contains(event.target as Node)) onModelMenuClose();
    };
    const closeOnEscape = (event: globalThis.KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      event.stopPropagation();
      onModelMenuClose();
      modelMenuButtonRef.current?.focus();
    };
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [modelMenuOpen, onModelMenuClose]);

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
        <button type="button" className={`app-name-button ${homeOpen ? "active" : ""}`} aria-label="홈으로 이동" aria-current={homeOpen ? "page" : undefined} onClick={onHome}>
          <svg className="app-mark" viewBox="0 0 48 48" aria-hidden="true"><rect x="9" y="9" width="30" height="30" rx="5" fill="var(--accent)"/><path d="M24 18.5c-2.2-1.6-4.8-2.1-7-2.1v12.4c2.2 0 4.8.5 7 2.1 2.2-1.6 4.8-2.1 7-2.1V16.4c-2.2 0-4.8.5-7 2.1z" fill="var(--app-mark-foreground)"/><rect x="3" y="15" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="3" y="27" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="41" y="15" width="4" height="6" rx="2" fill="var(--accent)"/><rect x="41" y="27" width="4" height="6" rx="2" fill="var(--accent)"/></svg>
          <span>Local LLM Wiki</span>
        </button>
        <IconButton icon={SquarePen} label="새 대화" onClick={onNew} />
      </div>
      <label className="search-wrap">
        <Search size={14} strokeWidth={2} aria-hidden="true" />
        <input ref={searchInputRef} className="search-input" type="search" aria-label="대화 검색" placeholder="대화 검색" value={searchValue ?? localSearch} onChange={(event) => changeSearch(event.target.value)} />
      </label>
      <div className="sidebar-primary-nav">
        <button type="button" className={`sidebar-nav-button ${homeOpen ? "active" : ""}`} aria-current={homeOpen ? "page" : undefined} onClick={onHome}>
          <Home size={15} aria-hidden="true" />홈
        </button>
      </div>
      <div className="sidebar-section-label">최근 대화</div>
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
        <div ref={modelMenuAnchorRef} className="runtime-menu-anchor">
          <button
            ref={modelMenuButtonRef}
            type="button"
            className={`runtime-summary-button ${modelMenuOpen ? "active" : ""}`}
            aria-label={`로컬 AI 상태: ${runtimeLabel}`}
            aria-expanded={modelMenuOpen}
            aria-controls="model-management-menu"
            onClick={onModelMenuToggle}
          >
            <span key={`icon-${readiness}`} className={`runtime-state-icon ${runtimeTone(readiness)}`} aria-hidden="true"><RuntimeSummaryIcon readiness={readiness} /></span>
            <span key={`copy-${readiness}`} className="runtime-summary-copy"><strong>{runtimeLabel}</strong><small>{runtimeDetail(readiness, modelName)}</small></span>
            <ChevronRight size={14} aria-hidden="true" />
          </button>
          {modelMenuOpen && modelMenu}
        </div>
        <button type="button" className={`sidebar-diagnostics-button ${diagnosticsOpen ? "active" : ""}`} aria-label="진단" title="진단" aria-current={diagnosticsOpen ? "page" : undefined} onClick={onDiagnostics}>
          <Activity size={16} aria-hidden="true" />
        </button>
      </div>
    </nav>
  );
}
