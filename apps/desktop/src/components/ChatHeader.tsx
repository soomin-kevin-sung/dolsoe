import { Box, ChevronDown, Eraser, LoaderCircle, SlidersHorizontal } from "lucide-react";
import type { Ref } from "react";
import type { RuntimeStatus } from "../services/runtime";
import { IconButton } from "./IconButton";

interface Props {
  title: string; modelName: string; modelState: RuntimeStatus; settingsOpen: boolean;
  settingsButtonRef: Ref<HTMLButtonElement>; resetButtonRef: Ref<HTMLButtonElement>; onSettings(): void; onReset(): void;
}

export function ChatHeader({ title, modelName, modelState, settingsOpen, settingsButtonRef, resetButtonRef, onSettings, onReset }: Props) {
  return (
    <header className="chat-header">
      <div className="chat-title">{title}</div><div className="header-spacer" />
      <button type="button" className={`model-chip ${modelState}`} aria-label="GGUF 모델 선택">
        {modelState === "loading" ? <LoaderCircle className="spin" size={14} /> : <Box size={14} />}
        <span className="model-name" data-model-name>{modelName}</span>
        {modelState === "loading" ? <span className="metrics-line live">64%</span> : <ChevronDown size={14} />}
      </button>
      <span className="header-divider" />
      <IconButton buttonRef={resetButtonRef} icon={Eraser} label="대화 초기화" onClick={onReset} />
      <IconButton buttonRef={settingsButtonRef} icon={SlidersHorizontal} label="설정" className={settingsOpen ? "active" : ""} aria-pressed={settingsOpen} onClick={onSettings} />
    </header>
  );
}
