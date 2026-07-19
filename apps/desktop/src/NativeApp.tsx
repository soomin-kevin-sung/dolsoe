import { useEffect, useMemo, useRef, useState } from "react";

import "./styles/tokens.css";
import "./App.css";
import { ChatHeader } from "./components/ChatHeader";
import { Composer } from "./components/Composer";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { MessageList } from "./components/MessageList";
import { NativeDiagnosticsView } from "./components/NativeDiagnosticsView";
import { NativeSettingsPanel } from "./components/NativeSettingsPanel";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { useConversationWorkspace } from "./hooks/useConversationWorkspace";
import type { StoredMessage } from "./services/conversationService";
import type { Message, MockStateName, RuntimeSnapshot, RuntimeStatus, Session } from "./services/runtime";

type DialogType = "reset" | "reload" | "delete";

function DesktopRequired() {
  return (
    <div className="app" data-app-state="desktop-required">
      <div className="workspace">
        <section className="conversation-shell">
          <header className="chat-header"><div className="chat-title">Local LLM Wiki</div></header>
          <main className="conversation" aria-label="대화">
            <div className="message-column">
              <div className="empty-state">
                <h2>데스크톱 앱에서 실행해야 합니다</h2>
                <p>네이티브 llama.cpp 런타임은 Tauri 데스크톱 프로세스에서만 사용할 수 있습니다.</p>
                <code className="desktop-command">npm --prefix apps/desktop run tauri -- dev</code>
              </div>
            </div>
          </main>
        </section>
      </div>
      <footer className="status-bar" role="status">
        <div className="status-left"><span className="status-dot error" /><span className="status-text">데스크톱 API 없음</span></div>
      </footer>
    </div>
  );
}

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

function formatUpdatedAt(timestamp: number): string {
  const elapsed = Date.now() - timestamp;
  if (elapsed < 60_000) return "방금";
  if (elapsed < 3_600_000) return `${Math.floor(elapsed / 60_000)}분 전`;
  if (elapsed < 86_400_000) return `${Math.floor(elapsed / 3_600_000)}시간 전`;
  return new Intl.DateTimeFormat("ko-KR", { month: "numeric", day: "numeric" }).format(timestamp);
}

function toMessage(message: StoredMessage): Message {
  return {
    id: message.id,
    role: message.role,
    content: message.content,
    status: message.role === "assistant" ? message.status : undefined,
    time: message.role === "user" ? formatUpdatedAt(message.createdAt) : undefined,
  };
}

function NativeWorkspace() {
  const workspace = useConversationWorkspace();
  const runtime = workspace.runtime;
  const { state } = runtime;
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [dialog, setDialog] = useState<DialogType | null>(null);
  const [dialogTargetId, setDialogTargetId] = useState<string | null>(null);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const resetButtonRef = useRef<HTMLButtonElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const composerInputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
    document.documentElement.dataset.theme = theme;
  }, []);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        void workspace.create();
      } else if (event.ctrlKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        searchInputRef.current?.focus();
      } else if (event.ctrlKey && event.key === ",") {
        event.preventDefault();
        setSettingsOpen((open) => !open);
      } else if (event.key === "Escape" && dialog) {
        setDialog(null);
      } else if (event.key === "Escape" && workspace.state.activeTurn) {
        void workspace.stop();
      } else if (event.key === "Escape" && settingsOpen) {
        setSettingsOpen(false);
        settingsButtonRef.current?.focus();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dialog, settingsOpen, workspace]);

  const current = workspace.current;
  const messages = current?.messages.map(toMessage) ?? [];
  const storageFailed = Boolean(workspace.state.storageError);
  const viewState: MockStateName = storageFailed
    ? "error"
    : state.phase === "ready" && messages.length === 0
      ? "empty"
      : state.phase;
  const runtimeStatus: RuntimeStatus = state.phase === "no-model" ? "none" : state.phase;
  const selectedRuntimePack = runtime.runtimePacks.find((pack) => pack.id === runtime.appliedRuntime?.packId);
  const sessions: Session[] = workspace.state.conversations.map((conversation) => ({
    id: conversation.id,
    title: conversation.title,
    meta: formatUpdatedAt(conversation.updatedAt),
    active: conversation.id === workspace.state.selectedConversationId,
    generating: conversation.id === workspace.state.activeTurn?.conversationId,
  }));
  const title = diagnosticsOpen ? "진단" : current?.title ?? "새 대화";
  const statusText = storageFailed
    ? "대화 저장소 오류"
    : state.phase === "no-model"
      ? "모델 없음"
      : state.phase === "loading"
        ? "모델 로딩 중"
        : workspace.state.activeTurn
          ? "생성 중"
          : state.phase === "error"
            ? "추론 오류"
            : "준비됨";
  const snapshot = useMemo<RuntimeSnapshot>(() => ({
    state: viewState,
    title,
    modelName: state.modelName,
    runtimeStatus,
    statusText,
    sessions,
    messages,
    telemetry: state.telemetry,
    packs: [{ id: "cpu", name: "CPU", version: "cpu-dev", status: "installed" }],
    settingsOpen,
    diagnosticsOpen,
    dialog: dialog === "delete" ? null : dialog,
  }), [diagnosticsOpen, dialog, messages, runtimeStatus, sessions, settingsOpen, state.modelName, state.telemetry, statusText, title, viewState]);

  function openConversationDialog(type: "reset" | "delete", conversationId: string) {
    setDialogTargetId(conversationId);
    setDialog(type);
  }

  async function confirmDialog() {
    if (dialog === "reload") {
      await runtime.reload();
    } else if (dialog === "reset" && dialogTargetId) {
      await workspace.clear(dialogTargetId);
    } else if (dialog === "delete" && dialogTargetId) {
      await workspace.remove(dialogTargetId);
    }
  }

  const dialogMessageCount = dialogTargetId
    ? workspace.state.details[dialogTargetId]?.messages.length ?? 0
    : 0;
  const composerDisabled = workspace.state.loading
    || storageFailed
    || Boolean(workspace.state.activeTurn)
    || ["no-model", "loading", "error"].includes(state.phase);

  return (
    <div className="app" data-app-state={viewState}>
      <div className="workspace">
        <Sidebar
          sessions={sessions}
          diagnosticsOpen={diagnosticsOpen}
          searchInputRef={searchInputRef}
          searchValue={workspace.state.search}
          onSearchChange={workspace.setSearch}
          onNew={() => void workspace.create()}
          onDiagnostics={() => setDiagnosticsOpen(true)}
          onSelect={(id) => { void workspace.select(id); setDiagnosticsOpen(false); }}
          onRename={async (id, nextTitle) => { await workspace.rename(id, nextTitle); }}
          onClear={(id) => openConversationDialog("reset", id)}
          onDelete={(id) => openConversationDialog("delete", id)}
        />
        <section className="conversation-shell">
          <ChatHeader
            title={title}
            modelName={state.modelName}
            modelState={runtimeStatus}
            settingsOpen={settingsOpen}
            settingsButtonRef={settingsButtonRef}
            resetButtonRef={resetButtonRef}
            onReset={() => current && openConversationDialog("reset", current.id)}
            onSettings={() => setSettingsOpen((open) => !open)}
            onModelSelect={() => void runtime.chooseModel()}
            onRename={current ? async (nextTitle) => { await workspace.rename(current.id, nextTitle); } : undefined}
            loadingProgress={state.loadingProgress}
          />
          <main className="conversation" aria-label="대화">
            {diagnosticsOpen
              ? <NativeDiagnosticsView state={state} runtimePack={selectedRuntimePack} />
              : <MessageList state={viewState} messages={messages} modelName={state.modelName} backend={state.backend} loadingProgress={state.loadingProgress} error={workspace.state.storageError ?? state.error} onChooseModel={() => void runtime.chooseModel()} />}
          </main>
          {!diagnosticsOpen && (
            <Composer
              disabled={composerDisabled}
              streaming={Boolean(workspace.state.activeTurn)}
              state={viewState}
              inputRef={composerInputRef}
              onSend={(prompt) => void workspace.submit(prompt)}
              onStop={() => void workspace.stop()}
            />
          )}
        </section>
        <NativeSettingsPanel
          open={settingsOpen}
          modelName={state.modelName}
          options={runtime.options}
          runtimePacks={runtime.runtimePacks}
          runtimePackError={runtime.runtimePackError}
          appliedRuntime={runtime.appliedRuntime}
          pendingRuntime={runtime.pendingRuntime}
          onOptionsChange={runtime.setOptions}
          onRuntimeChange={runtime.setPendingBackend}
          onApplyRuntime={() => void runtime.applyPendingRuntime()}
          onClose={() => { setSettingsOpen(false); settingsButtonRef.current?.focus(); }}
          onChooseModel={() => void runtime.chooseModel()}
          onUnload={() => void runtime.unload()}
          onReload={() => { setDialogTargetId(null); setDialog("reload"); }}
        />
      </div>
      <StatusBar snapshot={snapshot} />
      {dialog && (
        <ConfirmDialog
          type={dialog}
          messageCount={dialogMessageCount}
          onClose={() => setDialog(null)}
          onConfirm={() => void confirmDialog()}
          returnFocusRef={dialog === "reset" ? resetButtonRef : dialog === "reload" ? settingsButtonRef : undefined}
        />
      )}
    </div>
  );
}

export default function NativeApp() {
  return isTauriRuntime() ? <NativeWorkspace /> : <DesktopRequired />;
}
