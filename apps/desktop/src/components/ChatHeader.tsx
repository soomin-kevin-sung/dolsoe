import { Box, ChevronDown, Eraser, LoaderCircle, SlidersHorizontal } from "lucide-react";
import { useState, type KeyboardEvent, type Ref } from "react";
import type { RuntimeStatus } from "../services/runtime";
import { IconButton } from "./IconButton";

interface Props {
  title: string; modelName: string; modelState: RuntimeStatus; settingsOpen: boolean;
  settingsButtonRef: Ref<HTMLButtonElement>; resetButtonRef: Ref<HTMLButtonElement>; onSettings(): void; onReset(): void;
  onModelSelect?(): void; onRename?(title: string): void | Promise<void>; loadingProgress?: number | null;
}

export function ChatHeader({ title, modelName, modelState, settingsOpen, settingsButtonRef, resetButtonRef, onSettings, onReset, onModelSelect, onRename, loadingProgress }: Props) {
  const progress = Math.round((loadingProgress ?? 0.64) * 100);
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
      {renaming ? (
        <input autoFocus className="chat-title-input" aria-label="대화 이름" value={draft} onChange={(event) => setDraft(event.target.value)} onKeyDown={renameKeyDown} onBlur={commitRename} />
      ) : onRename ? (
        <button type="button" className="chat-title-button" onClick={() => { setDraft(title); setRenaming(true); }}>{title}</button>
      ) : <div className="chat-title">{title}</div>}
      <div className="header-spacer" />
      <button type="button" className={`model-chip ${modelState}`} aria-label="GGUF 모델 선택" onClick={onModelSelect}>
        {modelState === "loading" ? <LoaderCircle className="spin" size={14} /> : <Box size={14} />}
        <span className="model-name" data-model-name>{modelName}</span>
        {modelState === "loading" ? <span className="metrics-line live">{progress}%</span> : <ChevronDown size={14} />}
      </button>
      <span className="header-divider" />
      <IconButton buttonRef={resetButtonRef} icon={Eraser} label="대화 초기화" onClick={onReset} />
      <IconButton buttonRef={settingsButtonRef} icon={SlidersHorizontal} label="설정" className={settingsOpen ? "active" : ""} aria-pressed={settingsOpen} onClick={onSettings} />
    </header>
  );
}
