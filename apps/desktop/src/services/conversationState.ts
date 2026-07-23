import type {
  ConversationBootstrap,
  ConversationDetail,
  ConversationSummary,
  StartedTurn,
  TerminalMessageStatus,
} from "./conversationService";

export interface ActiveTurn {
  conversationId: string;
  assistantMessageId: string;
  requestHandle: string | null;
}

export interface ConversationState {
  conversations: ConversationSummary[];
  details: Record<string, ConversationDetail>;
  selectedConversationId: string | null;
  activeTurn: ActiveTurn | null;
  search: string;
  loading: boolean;
  storageError: string | null;
}

export type ConversationAction =
  | { type: "bootstrapped"; value: ConversationBootstrap }
  | { type: "draft-opened" }
  | { type: "selected"; detail: ConversationDetail }
  | { type: "summary-updated"; summary: ConversationSummary }
  | { type: "cleared"; detail: ConversationDetail }
  | { type: "deleted"; deletedId: string; fallback: ConversationDetail | null }
  | { type: "turn-started"; value: StartedTurn }
  | { type: "request-bound"; requestHandle: string }
  | { type: "turn-failed"; error: string }
  | { type: "token"; requestHandle: string; text: string }
  | { type: "terminal"; requestHandle: string; status: TerminalMessageStatus; tail: string }
  | { type: "search"; value: string }
  | { type: "storage-error"; error: string | null };

export function createConversationState(): ConversationState {
  return {
    conversations: [],
    details: {},
    selectedConversationId: null,
    activeTurn: null,
    search: "",
    loading: true,
    storageError: null,
  };
}

function sortConversations(values: ConversationSummary[]): ConversationSummary[] {
  return [...values].sort((left, right) =>
    right.updatedAt - left.updatedAt || right.id.localeCompare(left.id));
}

function upsertSummary(
  conversations: ConversationSummary[],
  summary: ConversationSummary,
): ConversationSummary[] {
  return sortConversations([
    summary,
    ...conversations.filter((candidate) => candidate.id !== summary.id),
  ]);
}

function withDetail(
  state: ConversationState,
  detail: ConversationDetail,
  select: boolean,
): ConversationState {
  return {
    ...state,
    conversations: upsertSummary(state.conversations, detail),
    details: { ...state.details, [detail.id]: detail },
    selectedConversationId: select ? detail.id : state.selectedConversationId,
    loading: false,
    storageError: null,
  };
}

function acceptsRequest(state: ConversationState, requestHandle: string): boolean {
  return Boolean(state.activeTurn)
    && (state.activeTurn?.requestHandle === null || state.activeTurn?.requestHandle === requestHandle);
}

function updateAssistant(
  state: ConversationState,
  update: (message: ConversationDetail["messages"][number]) => ConversationDetail["messages"][number],
): ConversationState {
  const active = state.activeTurn;
  if (!active) return state;
  const detail = state.details[active.conversationId];
  if (!detail) return state;
  return {
    ...state,
    details: {
      ...state.details,
      [detail.id]: {
        ...detail,
        messages: detail.messages.map((message) =>
          message.id === active.assistantMessageId ? update(message) : message),
      },
    },
  };
}

export function workspaceReducer(
  state: ConversationState,
  action: ConversationAction,
): ConversationState {
  switch (action.type) {
    case "bootstrapped":
      return {
        ...state,
        conversations: sortConversations(action.value.conversations),
        details: action.value.selected ? { [action.value.selected.id]: action.value.selected } : {},
        selectedConversationId: action.value.selected?.id ?? null,
        loading: false,
        storageError: null,
      };
    case "draft-opened":
      return { ...state, selectedConversationId: null, storageError: null };
    case "selected":
    case "cleared":
      return withDetail(state, action.detail, true);
    case "summary-updated": {
      const detail = state.details[action.summary.id];
      return {
        ...state,
        conversations: upsertSummary(state.conversations, action.summary),
        details: detail
          ? { ...state.details, [detail.id]: { ...detail, ...action.summary } }
          : state.details,
        storageError: null,
      };
    }
    case "deleted": {
      const details = { ...state.details };
      delete details[action.deletedId];
      if (action.fallback) details[action.fallback.id] = action.fallback;
      return {
        ...state,
        conversations: action.fallback
          ? upsertSummary(
              state.conversations.filter((item) => item.id !== action.deletedId),
              action.fallback,
            )
          : state.conversations.filter((item) => item.id !== action.deletedId),
        details,
        selectedConversationId: action.fallback?.id ?? null,
        storageError: null,
      };
    }
    case "turn-started": {
      const current = state.details[action.value.conversation.id] ?? {
        ...action.value.conversation,
        messages: [],
      };
      const detail = {
        ...current,
        ...action.value.conversation,
        messages: [...current.messages, action.value.user, action.value.assistant],
      };
      return {
        ...withDetail(state, detail, state.selectedConversationId === null),
        activeTurn: {
          conversationId: detail.id,
          assistantMessageId: action.value.assistant.id,
          requestHandle: null,
        },
      };
    }
    case "request-bound":
      if (!state.activeTurn
        || (state.activeTurn.requestHandle && state.activeTurn.requestHandle !== action.requestHandle)) {
        return state;
      }
      return { ...state, activeTurn: { ...state.activeTurn, requestHandle: action.requestHandle } };
    case "turn-failed": {
      const updated = updateAssistant(state, (message) => ({
        ...message,
        content: action.error,
        status: "error",
      }));
      return { ...updated, activeTurn: null };
    }
    case "token": {
      if (!acceptsRequest(state, action.requestHandle)) return state;
      const bound = state.activeTurn?.requestHandle
        ? state
        : { ...state, activeTurn: { ...state.activeTurn!, requestHandle: action.requestHandle } };
      return updateAssistant(bound, (message) => ({
        ...message,
        content: message.content + action.text,
        status: "streaming",
      }));
    }
    case "terminal": {
      if (!acceptsRequest(state, action.requestHandle)) return state;
      const bound = state.activeTurn?.requestHandle
        ? state
        : { ...state, activeTurn: { ...state.activeTurn!, requestHandle: action.requestHandle } };
      const updated = updateAssistant(bound, (message) => ({
        ...message,
        content: message.content + action.tail,
        status: action.status,
      }));
      return { ...updated, activeTurn: null };
    }
    case "search":
      return { ...state, search: action.value };
    case "storage-error":
      return { ...state, loading: false, storageError: action.error };
  }
}

export function selectVisibleConversations(state: ConversationState): ConversationSummary[] {
  const query = state.search.trim().toLocaleLowerCase();
  return query
    ? state.conversations.filter((conversation) => conversation.title.toLocaleLowerCase().includes(query))
    : state.conversations;
}

export function selectCurrentConversation(state: ConversationState): ConversationDetail | null {
  return state.selectedConversationId ? state.details[state.selectedConversationId] ?? null : null;
}
