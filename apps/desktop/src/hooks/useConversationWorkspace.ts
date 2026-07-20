import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
  ConversationService,
  type ConversationDetail,
  type TerminalMessageStatus,
} from "../services/conversationService";
import {
  createConversationState,
  selectCurrentConversation,
  selectVisibleConversations,
  workspaceReducer,
  type ConversationAction,
  type ConversationState,
} from "../services/conversationState";
import type { LlmEventDto } from "../services/nativeRuntime";
import { TokenDecoders } from "../services/nativeState";
import { chatMessagesForPrompt } from "../services/chatMessages";
import { restartAfterTerminalPersistence, TerminalWaiters } from "../services/terminalWaiters";
import { useNativeRuntime } from "./useNativeRuntime";

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function useConversationWorkspace() {
  const service = useMemo(() => new ConversationService(), []);
  const decoders = useRef(new TokenDecoders());
  const stateRef = useRef<ConversationState>(createConversationState());
  const terminalWaiters = useRef(new TerminalWaiters());
  const [state, setState] = useState(stateRef.current);

  const apply = useCallback((action: ConversationAction) => {
    setState((current) => {
      const next = workspaceReducer(current, action);
      stateRef.current = next;
      return next;
    });
  }, []);

  const onNativeEvent = useCallback((event: LlmEventDto) => {
    const handle = event.requestHandle;
    if (!handle) return;
    if (event.kind === "queued") {
      apply({ type: "request-bound", requestHandle: handle });
      return;
    }
    if (event.kind === "token") {
      const text = decoders.current.push(handle, event.bytes);
      apply({ type: "token", requestHandle: handle, text });
      return;
    }
    if (event.kind !== "done" && event.kind !== "cancelled" && event.kind !== "error") return;

    const current = stateRef.current;
    const active = current.activeTurn;
    if (!active || (active.requestHandle && active.requestHandle !== handle)) return;
    const detail = current.details[active.conversationId];
    const assistant = detail?.messages.find((message) => message.id === active.assistantMessageId);
    if (!assistant) return;
    let tail = decoders.current.finish(handle);
    if (event.kind === "error" && !assistant.content && !tail) {
      tail = new TextDecoder().decode(Uint8Array.from(event.bytes)) || `생성 오류 (${event.errorCode})`;
    }
    const status: TerminalMessageStatus = event.kind === "done" ? "complete" : event.kind;
    const content = assistant.content + tail;
    apply({ type: "terminal", requestHandle: handle, status, tail });
    void service.finishTurn(active.assistantMessageId, content, status)
      .then(() => terminalWaiters.current.resolveAll())
      .catch((error) => {
        const message = errorText(error);
        apply({ type: "storage-error", error: message });
        terminalWaiters.current.rejectAll(new Error(message));
      });
  }, [apply, service]);

  const runtime = useNativeRuntime(onNativeEvent);

  useEffect(() => {
    let disposed = false;
    void service.bootstrap()
      .then((value) => { if (!disposed) apply({ type: "bootstrapped", value }); })
      .catch((error) => { if (!disposed) apply({ type: "storage-error", error: errorText(error) }); });
    return () => {
      disposed = true;
      decoders.current.clear();
      terminalWaiters.current.resolveAll();
    };
  }, [apply, service]);

  const create = useCallback(async () => {
    try {
      const detail = await service.create();
      apply({ type: "created", detail });
      return detail;
    } catch (error) {
      apply({ type: "storage-error", error: errorText(error) });
      return null;
    }
  }, [apply, service]);

  const select = useCallback(async (conversationId: string) => {
    const cached = stateRef.current.details[conversationId];
    if (cached) {
      apply({ type: "selected", detail: cached });
      return;
    }
    try {
      apply({ type: "selected", detail: await service.load(conversationId) });
    } catch (error) {
      apply({ type: "storage-error", error: errorText(error) });
    }
  }, [apply, service]);

  const rename = useCallback(async (conversationId: string, title: string) => {
    try {
      const summary = await service.rename(conversationId, title);
      apply({ type: "summary-updated", summary });
      return true;
    } catch (error) {
      apply({ type: "storage-error", error: errorText(error) });
      return false;
    }
  }, [apply, service]);

  const cancelSource = useCallback(async (conversationId: string) => {
    const active = stateRef.current.activeTurn;
    if (!active || active.conversationId !== conversationId) return;
    const terminal = terminalWaiters.current.wait();
    try {
      await runtime.stop();
      await terminal.promise;
    } catch (error) {
      terminal.cancel();
      throw error;
    }
  }, [runtime]);

  const applyPendingRuntime = useCallback(async () => {
    const active = stateRef.current.activeTurn;
    try {
      if (active) await cancelSource(active.conversationId);
      await runtime.applyPendingRuntime();
    } catch (error) {
      runtime.reportError(error);
    }
  }, [cancelSource, runtime]);

  const restartApp = useCallback(async () => {
    const active = stateRef.current.activeTurn;
    try {
      await restartAfterTerminalPersistence(
        Boolean(active),
        async () => {
          if (active) await cancelSource(active.conversationId);
        },
        runtime.restartApp,
      );
    } catch (error) {
      runtime.reportError(error);
    }
  }, [cancelSource, runtime]);

  const workspaceRuntime = useMemo(
    () => ({ ...runtime, applyPendingRuntime, restartApp }),
    [applyPendingRuntime, restartApp, runtime],
  );

  const clear = useCallback(async (conversationId: string) => {
    try {
      await cancelSource(conversationId);
      const detail = await service.clear(conversationId);
      apply({ type: "cleared", detail });
      return true;
    } catch (error) {
      apply({ type: "storage-error", error: errorText(error) });
      return false;
    }
  }, [apply, cancelSource, service]);

  const remove = useCallback(async (conversationId: string) => {
    try {
      await cancelSource(conversationId);
      const fallback = await service.delete(conversationId);
      apply({ type: "deleted", deletedId: conversationId, fallback });
      return true;
    } catch (error) {
      apply({ type: "storage-error", error: errorText(error) });
      return false;
    }
  }, [apply, cancelSource, service]);

  const submit = useCallback(async (prompt: string) => {
    const current = selectCurrentConversation(stateRef.current);
    if (!current || Boolean(stateRef.current.activeTurn)) return;
    try {
      const chatMessages = chatMessagesForPrompt(current.messages, prompt);
      const turn = await service.startTurn(current.id, prompt);
      apply({ type: "turn-started", value: turn });
      try {
        const response = await runtime.submit(prompt, chatMessages);
        if (response) apply({ type: "request-bound", requestHandle: response.requestHandle });
      } catch (error) {
        const active = stateRef.current.activeTurn;
        if (active?.assistantMessageId === turn.assistant.id) {
          const message = errorText(error);
          apply({ type: "turn-failed", error: message });
          try {
            await service.finishTurn(turn.assistant.id, message, "error");
            terminalWaiters.current.resolveAll();
          } catch (storageError) {
            terminalWaiters.current.rejectAll(new Error(errorText(storageError)));
            throw storageError;
          }
        }
      }
    } catch (error) {
      apply({ type: "storage-error", error: errorText(error) });
    }
  }, [apply, runtime, service]);

  const setSearch = useCallback((value: string) => apply({ type: "search", value }), [apply]);
  const current = selectCurrentConversation(state);
  const visibleConversations = selectVisibleConversations(state);

  return {
    state,
    current,
    visibleConversations,
    runtime: workspaceRuntime,
    create,
    select,
    rename,
    clear,
    remove,
    submit,
    stop: workspaceRuntime.stop,
    setSearch,
  };
}

export type ConversationWorkspace = ReturnType<typeof useConversationWorkspace>;
export type { ConversationDetail };
