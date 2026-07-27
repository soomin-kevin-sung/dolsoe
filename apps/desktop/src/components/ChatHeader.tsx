import { Eraser, Home, SlidersHorizontal } from "lucide-react";
import { useState, type KeyboardEvent, type Ref } from "react";
import { DEFAULT_AGENT_MODE, type AgentModeId } from "../services/agentModes";
import { AgentModeMenu } from "./AgentModeMenu";
import { IconButton } from "./IconButton";

interface Props {
  title: string; settingsOpen: boolean;
  view: "home" | "chat" | "diagnostics";
  settingsButtonRef: Ref<HTMLButtonElement>; resetButtonRef: Ref<HTMLButtonElement>; onSettings(): void; onReset?(): void;
  onRename?(title: string): void | Promise<void>;
  agentMode?: AgentModeId;
  agentModeDisabled?: boolean;
  onAgentModeChange?(mode: AgentModeId): void;
  onOpenAgentSettings?(): void;
}

export function ChatHeader({
  title,
  view,
  settingsOpen,
  settingsButtonRef,
  resetButtonRef,
  onSettings,
  onReset,
  onRename,
  agentMode = DEFAULT_AGENT_MODE,
  agentModeDisabled = false,
  onAgentModeChange,
  onOpenAgentSettings,
}: Props) {
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState(title);

  function commitRename() {
    const value = draft.trim();
    if (value) void onRename?.(value);
    setRenaming(false);
  }

  function renameKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") { event.preventDefault(); commitRename(); }
    else if (event.key === "Escape") { event.preventDefault(); setDraft(title); setRenaming(false); }
  }

  return (
    <header className="chat-header">
      <div className="header-title-group">
      {renaming ? (
        <input autoFocus className="chat-title-input" aria-label="대화 이름" value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={renameKeyDown} onBlur={commitRename} />
      ) : onRename ? (
        <button type="button" className="chat-title-button" onClick={() => { setDraft(title); setRenaming(true); }}>{title}</button>
      ) : <div className={`chat-title ${view === "home" ? "home-title" : ""}`}>
        {view === "home" && <Home size={15} aria-hidden="true" />}
        <span>{title}</span>
      </div>}
      </div>
      <div className="header-spacer" />
      {view !== "diagnostics" && (
        <AgentModeMenu
          value={agentMode}
          disabled={agentModeDisabled}
          context={view === "home" ? "new-conversation" : "conversation"}
          onChange={onAgentModeChange}
          onOpenSettings={onOpenAgentSettings}
        />
      )}
      {view === "chat" && <><span className="header-divider" /><IconButton buttonRef={resetButtonRef} icon={Eraser} label="대화 초기화" onClick={onReset} disabled={!onReset} /></>}
      <IconButton buttonRef={settingsButtonRef} icon={SlidersHorizontal} label="설정" className={settingsOpen ? "active" : ""} aria-pressed={settingsOpen} onClick={onSettings} />
    </header>
  );
}
