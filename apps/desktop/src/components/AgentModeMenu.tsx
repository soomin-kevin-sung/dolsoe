import { Check, ChevronDown, SlidersHorizontal } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { AGENT_MODES, getAgentMode, type AgentModeId } from "../services/agentModes";
import { AgentModeIcon } from "./AgentModeIcon";

interface Props {
  value: AgentModeId;
  disabled?: boolean;
  context: "conversation" | "new-conversation";
  onChange?(mode: AgentModeId): void;
  onOpenSettings?(): void;
}

export function AgentModeMenu({ value, disabled = false, context, onChange, onOpenSettings }: Props) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const selectedMode = getAgentMode(value);

  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);

  useEffect(() => {
    if (!open) return;

    function closeOnOutsidePointer(event: PointerEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }

    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setOpen(false);
    }

    document.addEventListener("pointerdown", closeOnOutsidePointer);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePointer);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  return (
    <div className="agent-mode-menu-anchor" ref={rootRef}>
      <button
        type="button"
        className={`agent-mode-trigger ${open ? "active" : ""}`}
        aria-label={`에이전트 모드: ${selectedMode.label}`}
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={disabled}
        onClick={() => setOpen((current) => !current)}
      >
        <AgentModeIcon mode={value} size={15} strokeWidth={2} aria-hidden="true" />
        <span>{selectedMode.label}</span>
        <ChevronDown size={13} strokeWidth={2} aria-hidden="true" />
      </button>

      {open && (
        <div className="agent-mode-menu" role="menu" aria-label={context === "conversation" ? "현재 대화 에이전트 모드" : "새 대화 에이전트 모드"}>
          <div className="agent-mode-menu-heading">
            {context === "conversation" ? "현재 대화 모드" : "새 대화 모드"}
          </div>
          <div className="agent-mode-menu-options">
            {AGENT_MODES.map((mode) => {
              const selected = mode.id === value;
              const comingSoon = mode.availability === "coming-soon";
              return (
                <button
                  key={mode.id}
                  type="button"
                  role="menuitemradio"
                  aria-checked={selected}
                  className={selected ? "selected" : ""}
                  disabled={comingSoon}
                  onClick={() => {
                    onChange?.(mode.id);
                    setOpen(false);
                  }}
                >
                  <span className="agent-mode-menu-icon">
                    <AgentModeIcon mode={mode.id} size={17} strokeWidth={1.8} aria-hidden="true" />
                  </span>
                  <span className="agent-mode-menu-copy">
                    <span className="agent-mode-menu-title">
                      <strong>{mode.label}</strong>
                      {comingSoon && <span className="coming-soon-badge">준비 중</span>}
                    </span>
                    <small>{mode.description}</small>
                  </span>
                  {selected && <Check className="agent-mode-check" size={15} strokeWidth={2.2} aria-hidden="true" />}
                </button>
              );
            })}
          </div>
          {onOpenSettings && (
            <button
              type="button"
              role="menuitem"
              className="agent-mode-settings-link"
              onClick={() => {
                setOpen(false);
                onOpenSettings();
              }}
            >
              <SlidersHorizontal size={15} strokeWidth={2} aria-hidden="true" />
              <span>에이전트 설정</span>
            </button>
          )}
        </div>
      )}
    </div>
  );
}
