import { useEffect, useMemo, useRef, useState } from "react";

import "./styles/tokens.css";
import "./App.css";
import { ChatHeader } from "./components/ChatHeader";
import { Composer } from "./components/Composer";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { isCpuRuntimeRecoveryError, MessageList } from "./components/MessageList";
import { ModelMenu } from "./components/ModelMenu";
import { NativeDiagnosticsView } from "./components/NativeDiagnosticsView";
import { NativeHomeView } from "./components/NativeHomeView";
import { NativeSettingsPanel } from "./components/NativeSettingsPanel";
import type { SettingsTab } from "./components/SettingsDialog";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { useConversationWorkspace } from "./hooks/useConversationWorkspace";
import { readLastModelPath, rememberLastModelPath, useGeneralPreferences } from "./hooks/useGeneralPreferences";
import { useRuntimePackInstaller } from "./hooks/useRuntimePackInstaller";
import { useThemePreference } from "./hooks/useThemePreference";
import type { StoredMessage } from "./services/conversationService";
import { isCpuReady, readinessStatus, resolveHomeReadiness } from "./services/homeReadiness";
import type { Message, MockStateName, RuntimeSnapshot, RuntimeStatus, Session } from "./services/runtime";

type DialogType = "reset" | "reload" | "delete";

function DesktopRequired() {
  return (
    <div className="app" data-app-state="desktop-required">
      <div className="workspace">
        <section className="conversation-shell">
          <header className="chat-header"><div className="chat-title">돌쇠</div></header>
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
  const packInstaller = useRuntimePackInstaller();
  const [theme, setTheme] = useThemePreference();
  const [generalPreferences, updateGeneralPreferences] = useGeneralPreferences();
  const initialStartPage = useRef(generalPreferences.startPage);
  const initialAutoLoad = useRef(generalPreferences.autoLoadLastModel);
  const rememberedModelPath = useRef(readLastModelPath());
  const startupConversationResolved = useRef(false);
  const autoLoadAttempted = useRef(false);
  const { state } = runtime;
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("general");
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [homeOpen, setHomeOpen] = useState(initialStartPage.current === "home");
  const [diagnosticsOpen, setDiagnosticsOpen] = useState(false);
  const [dialog, setDialog] = useState<DialogType | null>(null);
  const [dialogTargetId, setDialogTargetId] = useState<string | null>(null);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const resetButtonRef = useRef<HTMLButtonElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const composerInputRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    if (settingsOpen || homeOpen) void packInstaller.refresh();
  }, [homeOpen, packInstaller.refresh, settingsOpen]);

  useEffect(() => {
    if (packInstaller.installState?.phase === "installed") void runtime.refreshRuntimePacks();
  }, [packInstaller.installState?.phase, runtime.refreshRuntimePacks]);

  useEffect(() => {
    if (state.phase !== "ready" || !state.modelPath) return;
    rememberedModelPath.current = state.modelPath;
    rememberLastModelPath(state.modelPath);
  }, [state.modelPath, state.phase]);

  useEffect(() => {
    if (!initialAutoLoad.current || autoLoadAttempted.current) return;
    if (state.phase !== "no-model" || !runtime.appliedRuntime) return;
    autoLoadAttempted.current = true;
    const modelPath = rememberedModelPath.current;
    if (modelPath) void runtime.applyConfiguration(runtime.options, runtime.appliedRuntime.backend, modelPath);
  }, [runtime.appliedRuntime, runtime.applyConfiguration, runtime.options, state.phase]);

  const current = workspace.current;
  const messages = current?.messages.map(toMessage) ?? [];
  const storageFailed = Boolean(workspace.state.storageError);
  const cpuRuntimeRecovery = state.phase === "error" && isCpuRuntimeRecoveryError(state.error);
  const cpuRuntimePack = runtime.runtimePacks.find((pack) => pack.id === "cpu");
  const availableCpuPack = packInstaller.availablePacks.find((pack) => pack.backend === "cpu");
  const readiness = resolveHomeReadiness({
    runtimePhase: state.phase,
    cpuPackStatus: cpuRuntimePack?.status,
    installState: packInstaller.installState,
    distributionLoading: packInstaller.loading,
    distributionError: packInstaller.error,
    runtimeRecovery: cpuRuntimeRecovery,
  });
  const homeStatus = readinessStatus(readiness);
  const modelSelectDisabled = !isCpuReady(readiness) || readiness === "model-loading";
  const viewState: MockStateName = storageFailed
    ? "error"
    : state.phase === "ready" && messages.length === 0
      ? "empty"
      : state.phase;
  const runtimeStatus: RuntimeStatus = readiness === "ready"
    ? workspace.state.activeTurn ? "streaming" : "ready"
    : homeStatus.tone;
  const selectedRuntimePack = runtime.runtimePacks.find((pack) => pack.id === runtime.appliedRuntime?.packId);
  const sessions: Session[] = workspace.state.conversations.map((conversation) => ({
    id: conversation.id,
    title: conversation.title,
    meta: formatUpdatedAt(conversation.updatedAt),
    active: !homeOpen && !diagnosticsOpen && conversation.id === workspace.state.selectedConversationId,
    generating: conversation.id === workspace.state.activeTurn?.conversationId,
  }));
  const title = homeOpen ? "홈" : diagnosticsOpen ? "진단" : current?.title ?? "새 대화";
  const statusText = storageFailed
    ? "대화 저장소 오류"
    : readiness === "ready" && workspace.state.activeTurn ? "생성 중" : homeStatus.text;
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

  useEffect(() => {
    if (startupConversationResolved.current || workspace.state.loading) return;
    startupConversationResolved.current = true;
    if (initialStartPage.current !== "last-conversation") return;
    setHomeOpen(!workspace.current);
    setDiagnosticsOpen(false);
  }, [workspace.current, workspace.state.loading]);

  function openConversationDialog(type: "reset" | "delete", conversationId: string) {
    setDialogTargetId(conversationId);
    setDialog(type);
  }

  function goHome() {
    setHomeOpen(true);
    setDiagnosticsOpen(false);
  }

  function openSettings(tab: SettingsTab = "general") {
    setModelMenuOpen(false);
    setSettingsTab(tab);
    setSettingsOpen(true);
  }

  function openDraft() {
    workspace.openDraft();
    setHomeOpen(false);
    setDiagnosticsOpen(false);
    requestAnimationFrame(() => composerInputRef.current?.focus());
  }

  function startConversationFlow() {
    if (readiness === "ready") openDraft();
    else goHome();
  }

  async function startPromptFromHome(prompt: string) {
    if (readiness !== "ready") return false;
    setHomeOpen(false);
    setDiagnosticsOpen(false);
    return workspace.startDraft(prompt);
  }

  function openDiagnostics() {
    setDiagnosticsOpen(true);
    setHomeOpen(false);
  }

  function selectConversation(id: string) {
    void workspace.select(id);
    setHomeOpen(false);
    setDiagnosticsOpen(false);
    requestAnimationFrame(() => composerInputRef.current?.focus());
  }

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        startConversationFlow();
      } else if (event.ctrlKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        searchInputRef.current?.focus();
      } else if (event.ctrlKey && event.key === ",") {
        event.preventDefault();
        setModelMenuOpen(false);
        if (settingsOpen) setSettingsOpen(false);
        else openSettings();
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
  });

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
          homeOpen={homeOpen}
          diagnosticsOpen={diagnosticsOpen}
          readiness={readiness}
          runtimeLabel={homeStatus.text}
          modelName={state.modelName}
          modelMenuOpen={modelMenuOpen}
          modelMenu={<ModelMenu
            open={modelMenuOpen}
            modelName={state.modelPath ? state.modelName : ""}
            modelPath={state.modelPath}
            runtimeLabel={homeStatus.text}
            backend={state.backend || runtime.appliedRuntime?.backend?.toUpperCase() || "런타임 없음"}
            actionsDisabled={modelSelectDisabled || Boolean(workspace.state.activeTurn)}
            onChooseModel={runtime.chooseModelPath}
            onReplace={(modelPath) => runtime.applyConfiguration(runtime.options, runtime.appliedRuntime?.backend ?? "cpu", modelPath)}
            onUnload={() => { setModelMenuOpen(false); void runtime.unload(); }}
            onOpenRuntime={() => openSettings("runtime")}
            onClose={() => setModelMenuOpen(false)}
          />}
          searchInputRef={searchInputRef}
          searchValue={workspace.state.search}
          onSearchChange={workspace.setSearch}
          onNew={startConversationFlow}
          onHome={goHome}
          onDiagnostics={openDiagnostics}
          onModelMenuToggle={() => setModelMenuOpen((current) => !current)}
          onModelMenuClose={() => setModelMenuOpen(false)}
          onSelect={selectConversation}
          onRename={async (id, nextTitle) => { await workspace.rename(id, nextTitle); }}
          onClear={(id) => openConversationDialog("reset", id)}
          onDelete={(id) => openConversationDialog("delete", id)}
        />
        <section className="conversation-shell">
          <ChatHeader
            title={title}
            view={homeOpen ? "home" : diagnosticsOpen ? "diagnostics" : "chat"}
            settingsOpen={settingsOpen}
            settingsButtonRef={settingsButtonRef}
            resetButtonRef={resetButtonRef}
            onReset={!homeOpen && !diagnosticsOpen && current ? () => openConversationDialog("reset", current.id) : undefined}
            onSettings={() => {
              if (settingsOpen) setSettingsOpen(false);
              else openSettings();
            }}
            onRename={!homeOpen && !diagnosticsOpen && current ? async (nextTitle) => { await workspace.rename(current.id, nextTitle); } : undefined}
          />
          <main className="conversation" aria-label={homeOpen ? "홈" : diagnosticsOpen ? "진단" : "대화"}>
            {homeOpen
              ? <NativeHomeView
                  readiness={readiness}
                  modelName={state.modelName}
                  backend={state.backend}
                  sessions={sessions}
                  cpuPack={availableCpuPack}
                  installState={packInstaller.installState}
                  modelProgress={state.loadingProgress}
                  onInstallCpu={() => void packInstaller.install("cpu")}
                  onRefreshCatalog={() => void packInstaller.refresh()}
                  onCancelInstall={() => void packInstaller.cancel()}
                  onRestart={() => void runtime.restartApp()}
                  onDismissInstall={packInstaller.dismiss}
                  onChooseModel={() => void runtime.chooseModel()}
                  onCancelModelLoad={() => void runtime.unload()}
                  onStartPrompt={startPromptFromHome}
                  onOpenDiagnostics={openDiagnostics}
                  onSelectSession={selectConversation}
                />
              : diagnosticsOpen
                ? <NativeDiagnosticsView state={state} runtimePack={selectedRuntimePack} />
                : <MessageList state={viewState} messages={messages} modelName={state.modelName} backend={state.backend} loadingProgress={state.loadingProgress} error={workspace.state.storageError ?? state.error} onChooseModel={() => void runtime.chooseModel()} onOpenSettings={() => openSettings("runtime")} />}
          </main>
          {!homeOpen && !diagnosticsOpen && (
            <Composer
              disabled={composerDisabled}
              streaming={Boolean(workspace.state.activeTurn)}
              state={viewState}
              runtimeRecovery={cpuRuntimeRecovery}
              inputRef={composerInputRef}
              onSend={workspace.submit}
              onStop={() => void workspace.stop()}
            />
          )}
        </section>
        <NativeSettingsPanel
          open={settingsOpen}
          initialTab={settingsTab}
          modelLoaded={Boolean(state.modelPath)}
          theme={theme}
          startPage={generalPreferences.startPage}
          autoLoadLastModel={generalPreferences.autoLoadLastModel}
          options={runtime.options}
          runtimePacks={runtime.runtimePacks}
          runtimePackError={runtime.runtimePackError}
          availableRuntimePacks={packInstaller.availablePacks}
          installState={packInstaller.installState}
          distributionError={packInstaller.error}
          distributionLoading={packInstaller.loading}
          appliedRuntime={runtime.appliedRuntime}
          reloadDisabled={Boolean(workspace.state.activeTurn) || readiness === "model-loading"}
          onThemeChange={setTheme}
          onStartPageChange={(startPage) => updateGeneralPreferences({ startPage })}
          onAutoLoadLastModelChange={(autoLoadLastModel) => updateGeneralPreferences({ autoLoadLastModel })}
          onOptionsChange={runtime.setOptions}
          onApplyConfiguration={runtime.applyConfiguration}
          onClose={() => { setSettingsOpen(false); settingsButtonRef.current?.focus(); }}
          onInstall={(packId) => void packInstaller.install(packId)}
          onCancelInstall={() => void packInstaller.cancel()}
          onRestart={() => void runtime.restartApp()}
          onDismissInstall={packInstaller.dismiss}
        />
      </div>
      <StatusBar snapshot={snapshot} compact={homeOpen || diagnosticsOpen} />
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
