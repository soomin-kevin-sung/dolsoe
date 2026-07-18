import { useEffect, useMemo, useRef, useState } from "react";
import "./styles/tokens.css";
import "./App.css";
import { ChatHeader } from "./components/ChatHeader";
import { Composer } from "./components/Composer";
import { MessageList } from "./components/MessageList";
import { Sidebar } from "./components/Sidebar";
import { StatusBar } from "./components/StatusBar";
import { DEFAULT_MODEL, mockRuntime } from "./services/mockRuntime";
import { parseMockState, type Message, type MockStateName } from "./services/runtime";

function queryValue(name: string) {
  return new URLSearchParams(window.location.search).get(name);
}

export default function App() {
  const initialState = parseMockState(queryValue("state"));
  const [state, setState] = useState<MockStateName>(initialState);
  const [settingsOpen, setSettingsOpen] = useState(["settings", "reload-confirm", "pack-install"].includes(initialState));
  const [extraMessages, setExtraMessages] = useState<Message[]>([]);
  const settingsButtonRef = useRef<HTMLButtonElement>(null);
  const snapshot = useMemo(() => mockRuntime.getSnapshot(state), [state]);
  const longModel = queryValue("longModel") === "1";
  const longMessage = queryValue("longMessage") === "1";
  const modelName = longModel ? "Q".repeat(80) : snapshot.modelName;
  const messages = useMemo(() => {
    const value = [...snapshot.messages, ...extraMessages];
    if (longMessage) {
      value.push({ id: "long", role: "assistant", content: "긴메시지".repeat(120), status: "complete" });
    }
    return value;
  }, [extraMessages, longMessage, snapshot.messages]);

  useEffect(() => {
    const requested = queryValue("theme");
    const theme = requested === "dark" ? "dark" : requested === "light" ? "light" : "light";
    document.documentElement.dataset.theme = theme;
  }, []);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        setState("empty");
        setExtraMessages([]);
      } else if (event.ctrlKey && event.key === ",") {
        event.preventDefault();
        setSettingsOpen((open) => !open);
      } else if (event.key === "Escape" && settingsOpen) {
        setSettingsOpen(false);
        settingsButtonRef.current?.focus();
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [settingsOpen]);

  function sendPrompt(prompt: string) {
    setExtraMessages((current) => [
      ...current,
      { id: `local-${current.length}`, role: "user", content: prompt, time: "방금" },
    ]);
  }

  return (
    <div className="app" data-app-state={state}>
      <div className="workspace">
        <Sidebar
          sessions={snapshot.sessions}
          diagnosticsOpen={false}
          onNew={() => setState("empty")}
        />
        <section className="conversation-shell">
          <ChatHeader
            title={snapshot.title}
            modelName={modelName || DEFAULT_MODEL}
            modelState={snapshot.runtimeStatus}
            settingsOpen={settingsOpen}
            settingsButtonRef={settingsButtonRef}
            onSettings={() => setSettingsOpen((open) => !open)}
          />
          <main className="conversation" aria-label="대화">
            <MessageList state={state} messages={messages} />
          </main>
          {state !== "diagnostics" && (
            <Composer
              disabled={["no-model", "loading", "error"].includes(state)}
              streaming={["streaming", "multi"].includes(state)}
              state={state}
              onSend={sendPrompt}
            />
          )}
        </section>
        <aside className="settings-placeholder" aria-label="설정" hidden={!settingsOpen}>
          <strong>설정</strong>
        </aside>
      </div>
      <StatusBar snapshot={{ ...snapshot, modelName }} />
    </div>
  );
}
