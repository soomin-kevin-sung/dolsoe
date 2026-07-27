export type AgentModeId = "chat" | "react" | "plan-and-solve";

export interface AgentModeDefinition {
  id: AgentModeId;
  label: string;
  description: string;
  availability: "available" | "coming-soon";
}

export const DEFAULT_AGENT_MODE: AgentModeId = "chat";

export const AGENT_MODES: readonly AgentModeDefinition[] = [
  {
    id: "chat",
    label: "Chat",
    description: "한 번의 모델 응답으로 대화를 이어갑니다.",
    availability: "available",
  },
  {
    id: "react",
    label: "ReAct",
    description: "필요할 때 도구를 사용하고 결과를 확인하며 답을 완성합니다.",
    availability: "available",
  },
  {
    id: "plan-and-solve",
    label: "Plan & Solve",
    description: "계획을 세운 뒤 각 단계를 실행하고 검토합니다.",
    availability: "coming-soon",
  },
];

export function getAgentMode(modeId: AgentModeId): AgentModeDefinition {
  return AGENT_MODES.find((mode) => mode.id === modeId) ?? AGENT_MODES[0];
}
