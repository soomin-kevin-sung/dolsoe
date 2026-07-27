import { Check } from "lucide-react";

import { AGENT_MODES, DEFAULT_AGENT_MODE, type AgentModeId } from "../services/agentModes";
import { AgentModeIcon } from "./AgentModeIcon";

interface Props {
  value?: AgentModeId;
  onChange?(mode: AgentModeId): void;
}

export function AgentModeSettings({ value = DEFAULT_AGENT_MODE, onChange }: Props) {
  return (
    <div id="settings-panel-agent" role="tabpanel">
      <section className="settings-section settings-section-first">
        <h3>새 대화 기본 모드</h3>
        <div className="agent-mode-settings-list" role="radiogroup" aria-label="새 대화 기본 에이전트 모드">
          {AGENT_MODES.map((mode) => {
            const selected = mode.id === value;
            const comingSoon = mode.availability === "coming-soon";
            return (
              <button
                key={mode.id}
                type="button"
                role="radio"
                aria-checked={selected}
                className={selected ? "selected" : ""}
                disabled={comingSoon}
                onClick={() => onChange?.(mode.id)}
              >
                <span className="agent-mode-settings-icon">
                  <AgentModeIcon mode={mode.id} size={18} strokeWidth={1.8} aria-hidden="true" />
                </span>
                <span className="agent-mode-settings-copy">
                  <span className="agent-mode-settings-title">
                    <strong>{mode.label}</strong>
                    {comingSoon && <span className="coming-soon-badge">준비 중</span>}
                  </span>
                  <small>{mode.description}</small>
                </span>
                <span className={`agent-mode-radio ${selected ? "selected" : ""}`} aria-hidden="true">
                  {selected && <Check size={12} strokeWidth={2.5} />}
                </span>
              </button>
            );
          })}
        </div>
        <p>새 대화를 만들 때 선택한 모드로 시작합니다. 진행 중인 대화의 모드는 헤더에서 따로 확인할 수 있습니다.</p>
      </section>
    </div>
  );
}
