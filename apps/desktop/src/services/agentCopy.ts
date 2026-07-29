export type AgentCopyState =
  | "thinking"
  | "choosing-tool"
  | "calculator"
  | "files"
  | "writing"
  | "completed"
  | "cancelled"
  | "failed";

const COPY: Record<AgentCopyState, readonly string[]> = {
  thinking: ["마당 쓰는 중…", "일머리 잡는 중…", "잠깐 궁리하는 중…"],
  "choosing-tool": ["연장 고르는 중…", "쓸 연장 챙기는 중…", "도구함 살펴보는 중…"],
  calculator: ["주판 튕기는 중…", "셈 맞춰보는 중…", "수를 헤아리는 중…"],
  files: ["서류 살피는 중…", "문서 뒤지는 중…", "장부 펼쳐보는 중…"],
  writing: ["글 다듬는 중…", "답을 갈무리하는 중…", "마무리 손질 중…"],
  completed: [
    "말끔히 마쳤습니다",
    "잘 마무리했습니다",
    "끝손질까지 마쳤습니다",
    "맡은 일 마쳤습니다",
  ],
  cancelled: ["하던 일을 멈췄습니다"],
  failed: ["일을 마치지 못했습니다"],
};

function stableHash(value: string): number {
  let hash = 0x811c9dc5;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

export function agentCopy(runId: string, state: AgentCopyState): string {
  const choices = COPY[state];
  return choices[stableHash(`${runId}:${state}`) % choices.length];
}
