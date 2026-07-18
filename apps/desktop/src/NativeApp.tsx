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
import { useNativeRuntime } from "./hooks/useNativeRuntime";
import type { MockStateName, RuntimeSnapshot, RuntimeStatus, Session } from "./services/runtime";

function DesktopRequired() {
  return <div className="app" data-app-state="desktop-required"><div className="workspace"><section className="conversation-shell"><header className="chat-header"><div className="chat-title">Local LLM Wiki</div></header><main className="conversation" aria-label="대화"><div className="message-column"><div className="empty-state"><h2>데스크톱 앱에서 실행해야 합니다</h2><p>네이티브 llama.cpp 런타임은 Tauri 데스크톱 프로세스에서만 사용할 수 있습니다.</p><code className="desktop-command">npm --prefix apps/desktop run tauri -- dev</code></div></div></main></section></div><footer className="status-bar" role="status"><div className="status-left"><span className="status-dot error" /><span className="status-text">네이티브 API 없음</span></div></footer></div>;
}

function isTauriRuntime() {
  return "__TAURI_INTERNALS__" in window;
}

function NativeWorkspace() {
  const runtime = useNativeRuntime();
  const { state } = runtime;
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [dialog, setDialog] = useState<"reset" | "reload" | null>(null);
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
      if (event.ctrlKey && event.key.toLowerCase() === "n") { event.preventDefault(); void runtime.reset(); }
      else if (event.ctrlKey && event.key.toLowerCase() === "f") { event.preventDefault(); searchInputRef.current?.focus(); }
      else if (event.ctrlKey && event.key === ",") { event.preventDefault(); setSettingsOpen((open) => !open); }
      else if (event.key === "Escape" && dialog) setDialog(null);
      else if (event.key === "Escape" && state.phase === "streaming") void runtime.stop();
      else if (event.key === "Escape" && settingsOpen) { setSettingsOpen(false); settingsButtonRef.current?.focus(); }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dialog, runtime, settingsOpen, state.phase]);

  const viewState: MockStateName = state.phase === "ready" && state.messages.length === 0 ? "empty" : state.phase;
  const runtimeStatus: RuntimeStatus = state.phase === "no-model" ? "none" : state.phase;
  const sessions: Session[] = [{ id: "native", title: state.messages.find((message) => message.role === "user")?.content || "새 대화", meta: "메모리", active: true, generating: state.phase === "streaming" }];
  const snapshot = useMemo<RuntimeSnapshot>(() => ({
    state: viewState,
    title: diagnosticsOpen ? "진단" : "새 대화",
    modelName: state.modelName,
    runtimeStatus,
    statusText: state.phase === "no-model" ? "모델 없음" : state.phase === "loading" ? "모델 로딩 중" : state.phase === "streaming" ? "생성 중" : state.phase === "error" ? "추론 오류" : "준비됨",
    sessions,
    messages: state.messages,
    telemetry: state.telemetry,
    packs: [{ id: "cpu", name: "CPU", version: "cpu-dev", status: "installed" }],
    settingsOpen,
    diagnosticsOpen,
    dialog,
  }), [diagnosticsOpen, dialog, runtimeStatus, sessions, settingsOpen, state, viewState]);

  return <div className="app" data-app-state={viewState}>
    <div className="workspace">
      <Sidebar sessions={sessions} diagnosticsOpen={diagnosticsOpen} searchInputRef={searchInputRef} onNew={() => void runtime.reset()} onDiagnostics={() => setDiagnosticsOpen(true)} onSelect={() => setDiagnosticsOpen(false)} />
      <section className="conversation-shell">
        <ChatHeader title={snapshot.title} modelName={state.modelName} modelState={runtimeStatus} settingsOpen={settingsOpen} settingsButtonRef={settingsButtonRef} resetButtonRef={resetButtonRef} onReset={() => setDialog("reset")} onSettings={() => setSettingsOpen((open) => !open)} onModelSelect={() => void runtime.chooseModel()} loadingProgress={state.loadingProgress} />
        <main className="conversation" aria-label="대화">{diagnosticsOpen ? <NativeDiagnosticsView state={state} /> : <MessageList state={viewState} messages={state.messages} modelName={state.modelName} backend={state.backend} loadingProgress={state.loadingProgress} error={state.error} onChooseModel={() => void runtime.chooseModel()} />}</main>
        {!diagnosticsOpen && <Composer disabled={["no-model", "loading", "error"].includes(state.phase)} streaming={state.phase === "streaming"} state={viewState} inputRef={composerInputRef} onSend={(prompt) => void runtime.submit(prompt)} onStop={() => void runtime.stop()} />}
      </section>
      <NativeSettingsPanel open={settingsOpen} modelName={state.modelName} options={runtime.options} onOptionsChange={runtime.setOptions} onClose={() => { setSettingsOpen(false); settingsButtonRef.current?.focus(); }} onChooseModel={() => void runtime.chooseModel()} onUnload={() => void runtime.unload()} onReload={() => setDialog("reload")} />
    </div>
    <StatusBar snapshot={snapshot} />
    {dialog && <ConfirmDialog type={dialog} onClose={() => setDialog(null)} onConfirm={dialog === "reset" ? () => void runtime.reset() : () => void runtime.reload()} returnFocusRef={dialog === "reset" ? resetButtonRef : settingsButtonRef} />}
  </div>;
}

export default function NativeApp() {
  return isTauriRuntime() ? <NativeWorkspace /> : <DesktopRequired />;
}
