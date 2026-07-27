import { ListChecks, MessageSquareText, Repeat2, type LucideProps } from "lucide-react";

import type { AgentModeId } from "../services/agentModes";

const modeIcons = {
  chat: MessageSquareText,
  react: Repeat2,
  "plan-and-solve": ListChecks,
} satisfies Record<AgentModeId, React.ComponentType<LucideProps>>;

interface Props extends LucideProps {
  mode: AgentModeId;
}

export function AgentModeIcon({ mode, ...props }: Props) {
  const Icon = modeIcons[mode];
  return <Icon {...props} />;
}
