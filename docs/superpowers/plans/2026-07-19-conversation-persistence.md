# Conversation Persistence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist conversation sessions and terminal messages in Rust-owned SQLite storage and connect the existing native chat UI to create, select, search, rename, clear, delete, and recover sessions.

**Architecture:** A cloneable `ConversationStore` wraps one `rusqlite::Connection` in a mutex and exposes transaction-oriented domain methods through Tauri commands. React uses a typed `ConversationService` and reducer-driven `useConversationWorkspace` hook; native request events are bound to persisted assistant message IDs so switching sessions cannot redirect output.

**Tech Stack:** Rust 1.93, rusqlite 0.39 with bundled SQLite, uuid 1.24, Tauri 2, React 19, TypeScript, Vitest, Playwright.

---

## File Map

- Create `apps/desktop/src-tauri/src/conversation_store.rs`: schema, migrations, validation, transactions, recovery, and store tests.
- Create `apps/desktop/src-tauri/src/conversation_commands.rs`: command DTOs and async Tauri adapters.
- Modify `apps/desktop/src-tauri/src/lib.rs`: open/manage the database and register commands.
- Modify `apps/desktop/src-tauri/Cargo.toml`: add `rusqlite` and `uuid`.
- Create `apps/desktop/src/services/conversationService.ts`: typed Tauri command client.
- Create `apps/desktop/src/services/conversationState.ts`: pure workspace reducer and token routing.
- Create `apps/desktop/src/services/conversationState.test.ts`: reducer tests.
- Create `apps/desktop/src/hooks/useConversationWorkspace.ts`: persistence/native orchestration.
- Modify `apps/desktop/src/hooks/useNativeRuntime.ts`: return submit responses and expose native event observation without changing command contracts.
- Modify `apps/desktop/src/NativeApp.tsx`: render persisted sessions/messages and CRUD dialogs.
- Modify `apps/desktop/src/components/Sidebar.tsx`: controlled search and per-session actions.
- Modify `apps/desktop/src/components/ChatHeader.tsx`: persisted title and rename entry point.
- Modify `apps/desktop/src/components/ConfirmDialog.tsx`: dynamic reset/delete message counts.
- Modify `apps/desktop/src/services/runtime.ts`: align UI message/session types with persisted DTOs.
- Modify `apps/desktop/src/App.css`: compact session menu and inline rename styles.
- Modify `apps/desktop/e2e/ui-states.spec.ts`: deterministic persisted-session interaction coverage in mock mode.
- Modify `docs/native-runtime-validation.md`: add persistence verification commands.

## Task 1: SQLite Schema And Bootstrap

**Files:**
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Create: `apps/desktop/src-tauri/src/conversation_store.rs`

- [ ] **Step 1: Add failing bootstrap tests**

Create store tests with these cases:

```rust
#[test]
fn bootstrap_migrates_recovers_and_creates_initial_conversation() {
    let store = ConversationStore::open_in_memory().unwrap();
    let first = store.bootstrap().unwrap();
    assert_eq!(first.conversations.len(), 1);
    assert_eq!(first.selected.messages.len(), 0);

    let turn = store.start_turn(&first.selected.id, "first prompt").unwrap();
    let second = store.bootstrap().unwrap();
    let recovered = second.selected.messages.iter().find(|m| m.id == turn.assistant.id).unwrap();
    assert_eq!(recovered.status, MessageStatus::Interrupted);
}

#[test]
fn bootstrap_is_repeatable_and_records_one_migration() {
    let store = ConversationStore::open_in_memory().unwrap();
    store.bootstrap().unwrap();
    store.bootstrap().unwrap();
    assert_eq!(store.migration_count().unwrap(), 1);
}
```

- [ ] **Step 2: Run the tests and verify RED**

Run:

```powershell
cargo test -p local-llm-wiki-desktop conversation_store::tests::bootstrap -- --nocapture
```

Expected: compilation fails because `ConversationStore` and its domain records do not exist.

- [ ] **Step 3: Add dependencies and minimal store**

Add:

```toml
rusqlite = { version = "0.39.0", features = ["bundled"] }
uuid = { version = "1.24.0", features = ["v4"] }
```

Define serializable domain records:

```rust
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationSummary {
    pub id: String,
    pub title: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum MessageStatus { Complete, Streaming, Cancelled, Interrupted, Error }

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct StoredMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: MessageRole,
    pub content: String,
    pub status: MessageStatus,
    pub created_at: i64,
    pub updated_at: i64,
}
```

Implement `ConversationStore { connection: Arc<Mutex<Connection>> }`, file and in-memory constructors, connection pragmas, migration 1 from the design, transactional recovery, initial conversation creation, ordered summary/message queries, and `bootstrap()`.

- [ ] **Step 4: Run focused and desktop tests**

```powershell
cargo test -p local-llm-wiki-desktop conversation_store -- --nocapture
cargo fmt --all -- --check
```

Expected: bootstrap tests pass and formatting is clean.

- [ ] **Step 5: Commit**

```powershell
git add Cargo.lock apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/src/conversation_store.rs
git commit -m "feat: add SQLite conversation bootstrap"
```

## Task 2: Conversation And Turn Transactions

**Files:**
- Modify: `apps/desktop/src-tauri/src/conversation_store.rs`

- [ ] **Step 1: Add failing CRUD and lifecycle tests**

Add tests proving:

```rust
#[test]
fn conversation_crud_and_turn_lifecycle_are_atomic() {
    let store = ConversationStore::open_in_memory().unwrap();
    let initial = store.bootstrap().unwrap().selected;
    let created = store.create_conversation().unwrap();
    store.rename_conversation(&created.id, "  renamed   chat ").unwrap();
    let turn = store.start_turn(&created.id, "  한국어   첫 질문  ").unwrap();
    assert_eq!(turn.conversation.title, "한국어 첫 질문");
    assert!(store.finish_turn(&turn.assistant.id, "answer", MessageStatus::Complete).unwrap());
    assert!(!store.finish_turn(&turn.assistant.id, "duplicate", MessageStatus::Complete).unwrap());
    store.clear_conversation(&created.id).unwrap();
    assert!(store.load_conversation(&created.id).unwrap().messages.is_empty());
    let fallback = store.delete_conversation(&created.id).unwrap();
    assert_eq!(fallback.id, initial.id);
}

#[test]
fn deleting_last_conversation_creates_a_fallback() {
    let store = ConversationStore::open_in_memory().unwrap();
    let only = store.bootstrap().unwrap().selected;
    let fallback = store.delete_conversation(&only.id).unwrap();
    assert_ne!(fallback.id, only.id);
}
```

Also test 40-character automatic title truncation, 80-character rename validation, unknown IDs, cascade delete, and rejection of non-terminal `finish_turn` statuses.

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -p local-llm-wiki-desktop conversation_store::tests -- --nocapture
```

Expected: missing CRUD methods fail compilation.

- [ ] **Step 3: Implement minimal transaction methods**

Implement these exact signatures:

```rust
pub fn create_conversation(&self) -> StoreResult<ConversationDetail>;
pub fn load_conversation(&self, id: &str) -> StoreResult<ConversationDetail>;
pub fn rename_conversation(&self, id: &str, title: &str) -> StoreResult<ConversationSummary>;
pub fn clear_conversation(&self, id: &str) -> StoreResult<ConversationDetail>;
pub fn delete_conversation(&self, id: &str) -> StoreResult<ConversationDetail>;
pub fn start_turn(&self, conversation_id: &str, prompt: &str) -> StoreResult<StartedTurn>;
pub fn finish_turn(&self, assistant_id: &str, content: &str, status: MessageStatus) -> StoreResult<bool>;
```

Use `chars().take(limit)` for Unicode-safe title limits, `split_whitespace().join(" ")` for normalization, `ON DELETE CASCADE`, and `UPDATE ... WHERE role = 'assistant' AND status = 'streaming'` for idempotent finalization.

- [ ] **Step 4: Verify store behavior**

```powershell
cargo test -p local-llm-wiki-desktop conversation_store::tests -- --nocapture
```

Expected: all store tests pass.

- [ ] **Step 5: Commit**

```powershell
git add apps/desktop/src-tauri/src/conversation_store.rs
git commit -m "feat: persist conversation lifecycle"
```

## Task 3: Tauri Conversation Commands

**Files:**
- Create: `apps/desktop/src-tauri/src/conversation_commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing command-contract tests**

Test serde field names and request validation:

```rust
#[test]
fn bootstrap_dto_uses_camel_case_contract() {
    let value = serde_json::to_value(fixture_bootstrap()).unwrap();
    assert!(value["selected"]["createdAt"].is_number());
    assert_eq!(value["selected"]["messages"][0]["conversationId"], "conversation-1");
}

#[test]
fn finish_request_rejects_streaming_status() {
    assert!(FinishTurnRequest::validate_status(MessageStatus::Streaming).is_err());
}
```

- [ ] **Step 2: Run and verify RED**

```powershell
cargo test -p local-llm-wiki-desktop conversation_commands -- --nocapture
```

- [ ] **Step 3: Implement commands and setup**

Create request DTOs with `#[serde(rename_all = "camelCase")]` and Tauri commands matching the seven names in the design. Reuse a generic `spawn_blocking` adapter. In `lib.rs`:

```rust
let app_data = app.path().app_local_data_dir()?;
std::fs::create_dir_all(&app_data)?;
app.manage(ConversationStore::open(app_data.join("local-llm-wiki.db"))?);
```

Register every `conversation_*` command alongside the existing `llm_*` commands.

- [ ] **Step 4: Verify Rust command layer**

```powershell
cargo test -p local-llm-wiki-desktop conversation_commands -- --nocapture
cargo check -p local-llm-wiki-desktop
```

- [ ] **Step 5: Commit**

```powershell
git add apps/desktop/src-tauri/src/conversation_commands.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: expose conversation persistence commands"
```

## Task 4: Frontend Service And Workspace Reducer

**Files:**
- Create: `apps/desktop/src/services/conversationService.ts`
- Create: `apps/desktop/src/services/conversationState.ts`
- Create: `apps/desktop/src/services/conversationState.test.ts`
- Modify: `apps/desktop/src/services/runtime.ts`

- [ ] **Step 1: Write failing service and reducer tests**

Cover command names and request shapes with injected bindings. Add reducer tests like:

```typescript
it("routes tokens to the bound conversation after selection changes", () => {
  let state = bootstrappedState(twoConversationFixture);
  state = workspaceReducer(state, { type: "turn-started", turn, conversationId: "a" });
  state = workspaceReducer(state, { type: "request-bound", requestHandle: "42" });
  state = workspaceReducer(state, { type: "selected", detail: conversationB });
  state = workspaceReducer(state, { type: "token", requestHandle: "42", text: "answer" });
  expect(state.details.a.messages.at(-1)?.content).toBe("answer");
  expect(state.details.b.messages).toEqual(conversationB.messages);
});

it("filters normalized titles without mutating persisted order", () => {
  const state = workspaceReducer(bootstrappedState(fixture), { type: "search", value: "  rust " });
  expect(selectVisibleConversations(state).map((item) => item.title)).toEqual(["Rust bridge"]);
  expect(state.conversations).toEqual(fixture.conversations);
});
```

- [ ] **Step 2: Run and verify RED**

```powershell
npm --prefix apps/desktop run test:unit -- conversationState
```

- [ ] **Step 3: Implement typed service and pure reducer**

`ConversationService` must expose:

```typescript
bootstrap(): Promise<ConversationBootstrap>;
create(): Promise<ConversationDetail>;
load(conversationId: string): Promise<ConversationDetail>;
rename(conversationId: string, title: string): Promise<ConversationSummary>;
clear(conversationId: string): Promise<ConversationDetail>;
delete(conversationId: string): Promise<ConversationDetail>;
startTurn(conversationId: string, prompt: string): Promise<StartedTurn>;
finishTurn(assistantMessageId: string, content: string, status: TerminalMessageStatus): Promise<boolean>;
```

The reducer stores summaries, details keyed by ID, selected ID, one pending/bound request, search text, loading state, and storage error. Token and terminal actions resolve the source conversation through the binding, never through the current selection.

- [ ] **Step 4: Verify unit tests and type checking**

```powershell
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
```

- [ ] **Step 5: Commit**

```powershell
git add apps/desktop/src/services/conversationService.ts apps/desktop/src/services/conversationState.ts apps/desktop/src/services/conversationState.test.ts apps/desktop/src/services/runtime.ts
git commit -m "feat: add persisted conversation frontend state"
```

## Task 5: Orchestrate Native Events And Persistence

**Files:**
- Create: `apps/desktop/src/hooks/useConversationWorkspace.ts`
- Modify: `apps/desktop/src/hooks/useNativeRuntime.ts`
- Modify: `apps/desktop/src/services/nativeState.test.ts`

- [ ] **Step 1: Add failing orchestration tests around race helpers**

Extract and test pure request-binding helpers for both event-before-submit-response and immediate-stop races:

```typescript
it("adopts a queued handle before submit resolves", () => {
  const state = workspaceReducer(pendingTurnState, { type: "native-handle-seen", requestHandle: "9" });
  expect(state.activeRequest?.requestHandle).toBe("9");
});

it("keeps a terminal update bound when another session is selected", () => {
  const state = reduceTerminal(selectedBWithRequestFromA, terminalEvent);
  expect(state.details.a.messages.at(-1)?.status).toBe("cancelled");
  expect(state.selectedConversationId).toBe("b");
});
```

- [ ] **Step 2: Run and verify RED**

```powershell
npm --prefix apps/desktop run test:unit -- conversationState nativeState
```

- [ ] **Step 3: Implement the workspace hook**

The hook must:

- bootstrap once before enabling session actions;
- subscribe to `llm://event` before reading status;
- call `conversation_start_turn` before `llm_submit`;
- bind queued/token/terminal events to the persisted assistant ID;
- use a request-specific `TextDecoder` and flush it on terminal;
- call `conversation_finish_turn` exactly once per terminal event;
- mark the assistant `error` when native submit rejects;
- cancel the source request before clear/delete of that source session;
- keep session selection available while globally disabling generation submission during one active request.

Change `useNativeRuntime.submit(prompt)` to return `Promise<SubmitResponse | undefined>` while retaining its immediate-cancel behavior. Add an optional event observer callback held in a ref so the workspace sees events without resubscribing on every render.

- [ ] **Step 4: Verify unit and build**

```powershell
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
```

- [ ] **Step 5: Commit**

```powershell
git add apps/desktop/src/hooks/useConversationWorkspace.ts apps/desktop/src/hooks/useNativeRuntime.ts apps/desktop/src/services/nativeState.test.ts
git commit -m "feat: bind native requests to persisted conversations"
```

## Task 6: Persisted Session UI

**Files:**
- Modify: `apps/desktop/src/NativeApp.tsx`
- Modify: `apps/desktop/src/components/Sidebar.tsx`
- Modify: `apps/desktop/src/components/ChatHeader.tsx`
- Modify: `apps/desktop/src/components/ConfirmDialog.tsx`
- Modify: `apps/desktop/src/App.css`
- Modify: `apps/desktop/e2e/ui-states.spec.ts`

- [ ] **Step 1: Add failing deterministic UI tests**

Keep `?state=` mocks independent from Tauri and add tests for controlled session UI primitives:

```typescript
test("session search filters and selection closes diagnostics", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.getByRole("searchbox", { name: "대화 검색" }).fill("Rust");
  await expect(page.locator(".session-item")).toHaveCount(1);
  await page.locator(".session-item").click();
  await expect(page.getByRole("form", { name: "메시지 입력" })).toBeVisible();
});

test("session actions expose rename clear and delete commands", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.getByRole("button", { name: /대화 메뉴/ }).first().click();
  await expect(page.getByRole("menuitem", { name: "이름 변경" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "대화 초기화" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "삭제" })).toBeVisible();
});
```

- [ ] **Step 2: Run and verify RED**

```powershell
npm --prefix apps/desktop run test:e2e -- --grep "session"
```

- [ ] **Step 3: Connect UI controls**

Replace the synthetic native session with workspace summaries/messages. Implement:

- controlled title search;
- `Ctrl+N` create/select;
- sidebar selection and generation indicator;
- compact `MoreHorizontal` session menu with accessible `menu`/`menuitem` roles;
- inline rename with Enter commit and Escape cancel;
- dynamic clear/delete confirmations with actual message counts;
- storage bootstrap/error empty states;
- global composer busy state while native generation is active.

Preserve existing mock behavior by passing optional callbacks and mock defaults rather than invoking SQLite from `MockApp`.

- [ ] **Step 4: Verify UI**

```powershell
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:e2e
```

- [ ] **Step 5: Commit**

```powershell
git add apps/desktop/src/NativeApp.tsx apps/desktop/src/components/Sidebar.tsx apps/desktop/src/components/ChatHeader.tsx apps/desktop/src/components/ConfirmDialog.tsx apps/desktop/src/App.css apps/desktop/e2e/ui-states.spec.ts
git commit -m "feat: add persisted conversation workflows"
```

## Task 7: Restart Smoke And Milestone Verification

**Files:**
- Modify: `docs/native-runtime-validation.md`

- [ ] **Step 1: Document the persistence smoke path**

Add exact steps:

```powershell
npm --prefix apps/desktop run tauri -- dev
```

Create two conversations, submit and stop one prompt, rename the other, close the app, restart, and verify both sessions plus the cancelled assistant content return. Start a generation, terminate the app before terminal, restart, and verify the assistant status is interrupted.

- [ ] **Step 2: Run fresh full verification**

```powershell
cargo fmt --all -- --check
cargo test -p llm-runtime --lib
cargo test --workspace --exclude llm-runtime
$env:LLW_TEST_RUNTIME = Join-Path $env:LOCALAPPDATA 'io.github.soomin-kevin-sung.local-llm-wiki\runtime-packs\cpu-dev\local_llm_runtime.dll'
cargo test -p llm-runtime --test fake_runtime -- --nocapture
cargo check -p local-llm-wiki-desktop
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:e2e
git diff --check
```

Expected: all Rust, native DLL integration, frontend unit, build, and Playwright checks pass.

- [ ] **Step 3: Run the actual Tauri restart smoke**

Use the installed `cpu-dev` runtime pack and a local test GGUF. Verify model load, token streaming, stop, session switch, title search, restart restoration, and interrupted recovery. Inspect the application at 1024x700 and a desktop viewport for overlap and clipping.

- [ ] **Step 4: Perform one milestone review**

Review the complete diff once, focusing on migration idempotency, transaction boundaries, mutex poisoning, command DTO consistency, request-event races, wrong-session token routing, and destructive confirmation behavior. Fix only Critical or Important findings and rerun affected tests.

- [ ] **Step 5: Commit validation documentation**

```powershell
git add docs/native-runtime-validation.md
git commit -m "docs: validate conversation persistence"
```

- [ ] **Step 6: Merge and publish using the user's standing choice**

Fast-forward `feature/conversation-persistence` into `main`, rerun the merged-result tests, push `origin/main`, and remove the feature worktree and branch.
