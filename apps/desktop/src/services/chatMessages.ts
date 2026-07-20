import type { ConversationDetail } from "./conversationService";
import type { SubmitRequest } from "./nativeRuntime";

export function chatMessagesForPrompt(
  messages: ConversationDetail["messages"],
  prompt: string,
): SubmitRequest["messages"] {
  const chat: SubmitRequest["messages"] = [];
  for (let index = 0; index + 1 < messages.length; index += 1) {
    const user = messages[index];
    const assistant = messages[index + 1];
    if (user.role !== "user" || user.status !== "complete"
      || assistant.role !== "assistant" || assistant.status !== "complete") continue;
    chat.push(
      { role: "user", content: user.content },
      { role: "assistant", content: assistant.content },
    );
    index += 1;
  }
  chat.push({ role: "user", content: prompt });
  return chat;
}
