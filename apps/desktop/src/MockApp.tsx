import { useEffect, useMemo, useRef, useState } from "react";
import "./styles/tokens.css";
import "./App.css";
import { ChatHeader } from "./components/ChatHeader";
import { Composer } from "./components/Composer";
import { MessageList } from "./components/MessageList";
import { ModelMenu } from "./components/ModelMenu";
import { NativeHomeView } from "./components/NativeHomeView";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { SettingsPanel } from "./components/SettingsPanel";
import type { SettingsTab } from "./components/SettingsDialog";
import { DiagnosticsView } from "./components/DiagnosticsView";
import { ConfirmDialog } from "./components/ConfirmDialog";
import type { HomeReadinessKind } from "./services/homeReadiness";
import { DEFAULT_MODEL, mockRuntime } from "./services/mockRuntime";
import { parseMockState, type Message, type MockStateName } from "./services/runtime";
import { useGeneralPreferences } from "./hooks/useGeneralPreferences";
import { useThemePreference } from "./hooks/useThemePreference";

function queryValue(name: string) {
  return new URLSearchParams(window.location.search).get(name);
}

export default function MockApp() {
  const initialState = parseMockState(queryValue("state"));
  const requestedTheme = queryValue("theme");
  const [theme, setTheme] = useThemePreference(requestedTheme === "dark" || requestedTheme === "light" ? requestedTheme : "system");
  const [generalPreferences, updateGeneralPreferences] = useGeneralPreferences();
  const [state, setState] = useState<MockStateName>(initialState);
  const [homeOpen, setHomeOpen] = useState(queryValue("view") === "home");
  const requestedSettingsTab = queryValue("tab");
  const [settingsOpen, setSettingsOpen] = useState(["settings", "reload-confirm", "pack-install"].includes(initialState));
  const [settingsTab, setSettingsTab] = useState<SettingsTab>(
    initialState === "pack-install" ? "runtime" : requestedSettingsTab === "agent" ? "agent" : "general",
  );
  const [modelMenuOpen, setModelMenuOpen] = useState(false);
  const [activeModelName, setActiveModelName] = useState(initialState === "no-model" ? "" : DEFAULT_MODEL);
  const [extraMessages, setExtraMessages] = useState<Message[]>([]);
  const [dialog, setDialog] = useState<"reset" | "reload" | null>(mockRuntime.getSnapshot(initialState).dialog);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const resetButtonRef = useRef<HTMLButtonElement>(null);
  const searchInputRef = useRef<HTMLInputElement>(null);
  const composerInputRef = useRef<HTMLTextAreaElement>(null);
  const snapshot = useMemo(() => mockRuntime.getSnapshot(state), [state]);
  const longModel = queryValue("longModel") === "1";
  const longMessage = queryValue("longMessage") === "1";
  const agentPreview = queryValue("agent");
  const modelName = longModel ? "Q".repeat(80) : activeModelName || snapshot.modelName;
  const readiness: HomeReadinessKind = state === "no-model"
    ? "model-missing"
    : state === "loading"
      ? "model-loading"
      : state === "error"
        ? "runtime-failed-unknown"
        : "ready";
  const messages = useMemo(() => {
    const value = [...snapshot.messages, ...extraMessages];
    if (longMessage) {
      value.push({ id: "long", role: "assistant", content: "긴메시지".repeat(120), status: "complete" });
    }
    if (agentPreview === "running" || agentPreview === "complete" || agentPreview === "single") {
      for (let index = value.length - 1; index >= 0; index -= 1) {
        if (value[index].role !== "assistant") continue;
        value[index] = {
          ...value[index],
          agentRun: {
            runId: "preview-react-run",
            assistantMessageId: value[index].id,
            mode: "react",
            status: agentPreview === "running" ? "running" : "complete",
            startedAt: 1,
            finishedAt: agentPreview === "running" ? null : 2,
            phase: agentPreview === "running" ? "choosing-tool" : undefined,
            tools: [
              {
                activityId: "preview-list-files",
                toolName: "list_files",
                status: "complete",
                input: ".",
                output: "Cargo.toml\npackage.json\napps/\ncrates/\nnative/",
                durationMs: 82,
              },
              ...(agentPreview === "single" ? [] : [{
                activityId: "preview-search-files",
                toolName: "search_files",
                status: agentPreview === "running" ? "running" as const : "complete" as const,
                input: '{"query":"bundle","path":"."}',
                output: agentPreview === "running" ? "" : "apps/desktop/src-tauri/tauri.conf.json",
                durationMs: agentPreview === "running" ? 0 : 1714,
              }]),
            ],
          },
        };
        break;
      }
    }
    return value;
  }, [agentPreview, extraMessages, longMessage, snapshot.messages]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        setHomeOpen(false);
        setState("empty");
        setExtraMessages([]);
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
      } else if (event.key === "Escape" && ["streaming", "multi"].includes(state)) {
        stopGeneration();
      } else if (event.key === "Escape" && settingsOpen) {
        setSettingsOpen(false);
        settingsButtonRef.current?.focus();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dialog, settingsOpen, state]);

  async function sendPrompt(prompt: string) {
    await mockRuntime.sendPrompt("quant", prompt);
    setExtraMessages((current) => [
      ...current,
      { id: `local-${current.length}`, role: "user", content: prompt, time: "방금" },
    ]);
    setState("streaming");
  }

  async function stopGeneration() {
    await mockRuntime.cancel("quant");
    setState("cancelled");
    requestAnimationFrame(() => composerInputRef.current?.focus());
  }

  function openSettings(tab: SettingsTab = "general") {
    setModelMenuOpen(false);
    setSettingsTab(tab);
    setSettingsOpen(true);
  }

  return (
    <div className="app" data-app-state={state}>
      <div className="workspace">
        <Sidebar
          sessions={snapshot.sessions}
          homeOpen={homeOpen}
          diagnosticsOpen={state === "diagnostics"}
          readiness={readiness}
          runtimeLabel={snapshot.statusText}
          modelName={modelName || DEFAULT_MODEL}
          modelMenuOpen={modelMenuOpen}
          modelMenu={<ModelMenu
            open={modelMenuOpen}
            modelName={state === "no-model" ? "" : modelName}
            modelPath={state === "no-model" ? null : `C:\\models\\${modelName}`}
            runtimeLabel={snapshot.statusText}
            backend="CUDA"
            actionsDisabled={state === "loading"}
            onChooseModel={async () => "C:\\models\\Llama-3.1-8B-Instruct-Q4_K_M.gguf"}
            onReplace={async (modelPath) => {
              const parts = modelPath.split(/[\\/]/);
              setActiveModelName(parts[parts.length - 1] ?? modelPath);
              setState("ready");
              return true;
            }}
            onUnload={() => { setModelMenuOpen(false); setActiveModelName(""); setState("no-model"); }}
            onOpenRuntime={() => openSettings("runtime")}
            onClose={() => setModelMenuOpen(false)}
          />}
          searchInputRef={searchInputRef}
          onNew={() => { setHomeOpen(false); setState("empty"); }}
          onHome={() => setHomeOpen(true)}
          onDiagnostics={() => { setHomeOpen(false); setState("diagnostics"); }}
          onModelMenuToggle={() => setModelMenuOpen((current) => !current)}
          onModelMenuClose={() => setModelMenuOpen(false)}
          onSelect={() => { setHomeOpen(false); setState("ready"); }}
        />
        <section className="conversation-shell">
          <ChatHeader
            title={homeOpen ? "홈" : snapshot.title}
            view={homeOpen ? "home" : state === "diagnostics" ? "diagnostics" : "chat"}
            settingsOpen={settingsOpen}
            settingsButtonRef={settingsButtonRef}
            resetButtonRef={resetButtonRef}
            onReset={() => setDialog("reset")}
            onSettings={() => {
              if (settingsOpen) setSettingsOpen(false);
              else openSettings();
            }}
            onOpenAgentSettings={() => openSettings("agent")}
          />
          <main className="conversation" aria-label={homeOpen ? "홈" : "대화"}>
            {homeOpen ? <NativeHomeView
              readiness={readiness}
              modelName={modelName || DEFAULT_MODEL}
              backend="CUDA"
              sessions={snapshot.sessions}
              installState={null}
              modelProgress={null}
              onInstallCpu={() => undefined}
              onRefreshCatalog={() => undefined}
              onCancelInstall={() => undefined}
              onRestart={() => undefined}
              onDismissInstall={() => undefined}
              onChooseModel={() => undefined}
              onCancelModelLoad={() => undefined}
              onStartPrompt={async (prompt) => { setHomeOpen(false); await sendPrompt(prompt); }}
              onOpenDiagnostics={() => { setHomeOpen(false); setState("diagnostics"); }}
              onSelectSession={() => { setHomeOpen(false); setState("ready"); }}
            /> : state === "diagnostics" ? <DiagnosticsView /> : <MessageList state={state} messages={messages} />}
          </main>
          {!homeOpen && state !== "diagnostics" && (
            <Composer
              disabled={["no-model", "loading", "error"].includes(state)}
              streaming={["streaming", "multi"].includes(state)}
              state={state}
              inputRef={composerInputRef}
              onSend={sendPrompt}
              onStop={stopGeneration}
            />
          )}
        </section>
        <SettingsPanel
          open={settingsOpen}
          initialTab={settingsTab}
          packs={snapshot.packs}
          theme={theme}
          startPage={generalPreferences.startPage}
          autoLoadLastModel={generalPreferences.autoLoadLastModel}
          onThemeChange={setTheme}
          onStartPageChange={(startPage) => updateGeneralPreferences({ startPage })}
          onAutoLoadLastModelChange={(autoLoadLastModel) => updateGeneralPreferences({ autoLoadLastModel })}
          onClose={() => { setSettingsOpen(false); settingsButtonRef.current?.focus(); }}
        />
      </div>
      <StatusBar snapshot={{ ...snapshot, modelName }} compact={homeOpen} />
      {dialog && <ConfirmDialog type={dialog} onClose={() => setDialog(null)} returnFocusRef={dialog === "reset" ? resetButtonRef : settingsButtonRef} />}
    </div>
  );
}
