# CPU Native UI Bridge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect the normal Tauri desktop UI to the managed CPU llama.cpp runtime pack for local GGUF loading, token streaming, cancellation, and metrics while preserving every query-driven mock state.

**Architecture:** A bounded command channel feeds one dedicated Rust worker thread that exclusively owns `InferenceRuntime`, `Model`, and `RequestStream`. Tauri commands exchange typed DTOs with that worker and emit byte-preserving `llm://event` events; the React entry point selects the existing mock app for `?state=` URLs and a native reducer/service for normal Tauri URLs.

**Tech Stack:** Rust 1.93, Tauri 2, crossbeam-channel, serde, React 19, TypeScript 5.8, Vitest, Playwright, PowerShell, CMake, llama.cpp C ABI runtime pack

---

## File Map

```text
apps/desktop/src-tauri/src/runtime_path.rs   Shared trusted runtime-pack path validation
apps/desktop/src-tauri/src/llm_dto.rs       Tauri command/event/status/metrics DTOs
apps/desktop/src-tauri/src/llm_worker.rs    Bounded worker queue, native ownership, event relay
apps/desktop/src-tauri/src/llm_commands.rs  Thin async Tauri command boundary
apps/desktop/src-tauri/src/runtime_probe.rs Runtime probing through shared path resolver
apps/desktop/src-tauri/src/lib.rs           Plugins, worker setup, handlers, shutdown
apps/desktop/src/services/nativeRuntime.ts  Injectable invoke/listen/dialog adapter
apps/desktop/src/services/nativeState.ts    Pure native reducer and streamed UTF-8 decoder
apps/desktop/src/MockApp.tsx                 Existing query-fixture app moved without behavior changes
apps/desktop/src/NativeApp.tsx               Native model/chat lifecycle orchestration
apps/desktop/src/App.tsx                     Mock/native mode switch only
apps/desktop/src/components/*.tsx            Reusable native callbacks and dynamic copy/options
scripts/prepare-dev-cpu-pack.ps1             Build, verify, and stage managed cpu-dev pack
```

### Task 1: Shared Runtime Path And DTO Contracts

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_path.rs`
- Create: `apps/desktop/src-tauri/src/llm_dto.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_probe.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Test: inline Rust unit tests in the new modules

- [ ] **Step 1: Write failing trusted-path and serialization tests**

Add tests proving that `RuntimePackResolver::resolve("cpu-dev")` stays under the canonical trusted root, rejects traversal/junction escapes, and reports the expected missing-pack path. Add DTO tests with these exact externally visible shapes:

```rust
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmMetricsDto {
    pub prompt_tokens: String,
    pub generated_tokens: String,
    pub cancelled_requests: String,
    pub failed_requests: String,
    pub queue_wait_nanoseconds: String,
    pub decode_nanoseconds: String,
    pub tokens_per_second: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LlmEventDto {
    pub kind: LlmEventKind,
    pub request_handle: Option<String>,
    pub sequence_number: String,
    pub bytes: Vec<u8>,
    pub error_code: i32,
    pub metrics: Option<LlmMetricsDto>,
}
```

Verify `requestHandle`, `sequenceNumber`, and all 64-bit metric counters serialize as strings.

- [ ] **Step 2: Run the focused tests and observe RED**

Run: `cargo test -p local-llm-wiki-desktop`

Expected: FAIL because both modules and their types are missing.

- [ ] **Step 3: Extract the resolver and implement DTO conversion**

Move `MAX_RUNTIME_PACK_ID_LEN`, `validate_runtime_pack_id`, `runtime_library_filename`, and `resolve_runtime_library` into `runtime_path.rs`. Expose only:

```rust
#[derive(Clone)]
pub struct RuntimePackResolver { runtime_root: PathBuf }

impl RuntimePackResolver {
    pub fn new(runtime_root: PathBuf) -> Self;
    pub fn runtime_root(&self) -> &Path;
    pub fn resolve(&self, runtime_pack_id: &str) -> Result<PathBuf, String>;
}
```

Define `LlmPhase` (`NoModel`, `Loading`, `Ready`, `Streaming`, `Error`), `LlmStatusDto`, `LoadModelRequest`, `SubmitRequest`, `SubmitResponse`, `LlmMetricsDto`, `LlmEventKind`, and `LlmEventDto`. Use `String` for native `u64` values crossing IPC. `SubmitRequest` fields are `prompt`, `max_new_tokens`, `temperature`, `top_p`, and `seed`; defaults are supplied in TypeScript, not by optional Rust fields.

- [ ] **Step 4: Run focused and existing resolver tests**

Run: `cargo test -p local-llm-wiki-desktop`

Expected: PASS with the existing eight probe behaviors plus the new DTO tests.

- [ ] **Step 5: Commit**

```powershell
git add apps/desktop/src-tauri/src docs/superpowers/specs/2026-07-18-cpu-native-ui-bridge-design.md
git commit -m "feat: define native LLM command contracts"
```

### Task 2: Dedicated Native Worker And Tauri Commands

**Files:**
- Create: `apps/desktop/src-tauri/src/llm_worker.rs`
- Create: `apps/desktop/src-tauri/src/llm_commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `Cargo.lock`
- Test: inline unit tests in `llm_worker.rs` and `llm_commands.rs`

- [ ] **Step 1: Write failing worker state tests**

Define tests against a pure `WorkerGuard` API and require these transitions:

```rust
let mut guard = WorkerGuard::default();
assert_eq!(guard.begin_submit(), Err(WorkerError::ModelNotLoaded));
guard.begin_load()?;
guard.finish_load()?;
let handle = guard.begin_submit_with_handle(42)?;
assert_eq!(handle, 42);
assert_eq!(guard.begin_submit_with_handle(43), Err(WorkerError::Busy));
assert!(guard.cancel(42)?.is_first_request);
assert!(!guard.cancel(42)?.is_first_request);
guard.finish_request(42)?;
assert_eq!(guard.phase(), LlmPhase::Ready);
```

Also test a bounded channel capacity of 32, decimal-string cancel parsing, model extension validation, and terminal removal restoring `Ready`.

- [ ] **Step 2: Run worker tests and observe RED**

Run: `cargo test -p local-llm-wiki-desktop`

Expected: FAIL because the worker and commands do not exist.

- [ ] **Step 3: Implement the worker with native ownership**

Create `WorkerCommand` variants `GetStatus`, `LoadModel`, `UnloadModel`, `Submit`, `Cancel`, `GetMetrics`, and `Shutdown`. Each carries a bounded response sender. `WorkerHandle::spawn(resolver, event_sink)` creates `crossbeam_channel::bounded(32)` and starts named thread `llm-worker`.

Inside the thread, keep exactly:

```rust
struct NativeState {
    runtime: Option<InferenceRuntime>,
    model: Option<Model>,
    request: Option<RequestStream>,
    request_terminal: Option<Receiver<RuntimeEvent>>,
    relay: Option<EventRelayHandle>,
    status: LlmStatusDto,
}
```

Load the runtime with `RuntimeOptions { slot_count: 1, request_queue_capacity: 16, event_queue_capacity: 1024 }`. Load the model with `Backend::Cpu`, context 4096, logical batch 512, physical batch 128, GPU layers 0, mmap enabled, and both thread counts set to `available_parallelism().clamp(1, 8)`. Reject non-files and non-`.gguf` extensions before native calls.

After runtime load, move only the cloned `runtime.events()` receiver to a named `llm-event-relay` thread. The relay blocking-selects regular runtime events and a bounded terminal-control channel. Map `ModelProgress`, `Queued`, `Token`, `Metrics`, `Done`, `Cancelled`, and `Error`; ignore `Log` in this UI phase. The worker uses `recv_timeout(Duration::from_millis(5))` to drain the one terminal receiver and relay-failure notifications. When terminal arrives, send it to the relay; the relay drains regular events first, emits terminal, and acknowledges cleanup so token ordering is stable. On event sink failure, notify the worker, cancel the active request, and keep draining to terminal.

- [ ] **Step 4: Implement thin async Tauri commands**

Expose exactly:

```rust
llm_get_status(state: State<'_, WorkerHandle>) -> Result<LlmStatusDto, String>
llm_load_model(state: State<'_, WorkerHandle>, request: LoadModelRequest) -> Result<LlmStatusDto, String>
llm_unload_model(state: State<'_, WorkerHandle>) -> Result<LlmStatusDto, String>
llm_submit(state: State<'_, WorkerHandle>, request: SubmitRequest) -> Result<SubmitResponse, String>
llm_cancel(state: State<'_, WorkerHandle>, request_handle: String) -> Result<(), String>
llm_get_metrics(state: State<'_, WorkerHandle>) -> Result<LlmMetricsDto, String>
```

Run blocking channel waits through `tauri::async_runtime::spawn_blocking`. Register `tauri-plugin-dialog`, construct the resolver from `app_local_data_dir()/runtime-packs` in `Builder::setup`, manage the worker, and add all six commands to `generate_handler!`. `WorkerHandle::drop` sends `Shutdown` and joins without detaching.

- [ ] **Step 5: Run Rust tests and checks**

Run: `cargo test -p local-llm-wiki-desktop && cargo check -p local-llm-wiki-desktop`

Expected: PASS; no native DLL is needed because unit tests exercise guards, DTOs, and command parsing.

- [ ] **Step 6: Commit**

```powershell
git add Cargo.lock apps/desktop/src-tauri
git commit -m "feat: add native LLM worker commands"
```

### Task 3: Native TypeScript Service And Reducer

**Files:**
- Create: `apps/desktop/src/services/nativeRuntime.ts`
- Create: `apps/desktop/src/services/nativeRuntime.test.ts`
- Create: `apps/desktop/src/services/nativeState.ts`
- Create: `apps/desktop/src/services/nativeState.test.ts`
- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/package-lock.json`

- [ ] **Step 1: Add Vitest and write failing adapter tests**

Add script `"test:unit": "vitest run"` and dev dependency `vitest`. Inject rather than globally mock Tauri:

```ts
export interface NativeBindings {
  invoke<T>(command: string, args?: Record<string, unknown>): Promise<T>;
  listen<T>(event: string, handler: (event: { payload: T }) => void): Promise<() => void>;
  openGguf(): Promise<string | null>;
}
```

Tests must assert the six exact command names, camelCase request wrappers, `llm://event` subscription cleanup, and that cancel forwards the string handle unchanged.

- [ ] **Step 2: Write failing reducer/UTF-8 tests**

Feed the UTF-8 bytes for `한글` split inside both three-byte characters across several token events. Assert one assistant message becomes exactly `한글`, terminal flush marks it complete, repeated terminal events are ignored, cancel marks it cancelled, and stale request handles cannot append bytes.

- [ ] **Step 3: Run unit tests and observe RED**

Run: `npm run test:unit`

Expected: FAIL because native service and reducer modules are missing.

- [ ] **Step 4: Implement the service and pure state machine**

`NativeRuntimeService` wraps injected bindings and exports `getStatus`, `loadModel`, `unloadModel`, `submit`, `cancel`, `getMetrics`, `subscribe`, and `chooseModel`. Production bindings use `@tauri-apps/api/core`, `@tauri-apps/api/event`, and `@tauri-apps/plugin-dialog`.

The reducer state contains one memory-only session, messages, phase, model name/path, active request string, loading progress, error, and display telemetry. Keep `TextDecoder` instances outside serializable reducer state in a request-handle keyed `TokenDecoder` helper with `push(handle, bytes)`, `finish(handle)`, and `remove(handle)`.

- [ ] **Step 5: Run TypeScript unit tests and build**

Run: `npm run test:unit && npm run build`

Expected: PASS with no TypeScript errors.

- [ ] **Step 6: Commit**

```powershell
git add apps/desktop/package.json apps/desktop/package-lock.json apps/desktop/src/services
git commit -m "feat: add native runtime frontend state"
```

### Task 4: Connect The Existing Desktop Shell

**Files:**
- Create: `apps/desktop/src/MockApp.tsx`
- Create: `apps/desktop/src/NativeApp.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/components/ChatHeader.tsx`
- Modify: `apps/desktop/src/components/MessageList.tsx`
- Modify: `apps/desktop/src/components/SettingsPanel.tsx`
- Modify: `apps/desktop/src/components/OptionRow.tsx`
- Modify: `apps/desktop/src/components/StatusBar.tsx`
- Modify: `apps/desktop/src/services/runtime.ts`
- Modify: `apps/desktop/src/App.css`
- Test: `apps/desktop/e2e/ui-states.spec.ts`

- [ ] **Step 1: Preserve mock behavior before moving code**

Move the current `App.tsx` body unchanged to `MockApp.tsx`. Make `App.tsx` select `MockApp` only when `new URLSearchParams(location.search).has("state")`; otherwise select `NativeApp`. Run all 34 Playwright tests before adding native UI and confirm they remain green.

- [ ] **Step 2: Write failing normal-browser fallback test**

Add a Playwright case for `/` outside Tauri asserting the visible heading `데스크톱 앱에서 실행해야 합니다` and the command `npm --prefix apps/desktop run tauri -- dev`. This test must fail while `NativeApp` is absent.

- [ ] **Step 3: Implement `NativeApp` orchestration**

On mount subscribe before calling `getStatus`. Model selection uses the dialog, sends `runtimePackId: "cpu-dev"`, and updates loading progress from events. Send inserts user and empty streaming assistant messages before invoking submit. Stop and `Esc` cancel only the active string handle. Model replacement cancels, waits for terminal through state, unloads, then opens the picker. New/reset clears only memory messages.

Reuse the shell components by adding explicit callbacks and dynamic props: model picker click, model progress, no-model action, CPU-only settings, generation values, and telemetry. Change `OptionRow` to accept controlled `value` and `onChange` props while retaining `initial` as the mock-mode fallback. Bind max tokens, temperature, top-p, and seed to native submit options with defaults 256, 0.8, 0.95, and -1 (`u32::MAX` at the Rust boundary). Do not expose CUDA/Vulkan as selectable in native mode. Retain their existing mock rendering through props from `MockApp`.

- [ ] **Step 4: Implement non-Tauri fallback**

Detect missing `window.__TAURI_INTERNALS__` before constructing production bindings. Render the specified desktop-only error state inside the existing shell rather than throwing or leaving a blank page.

- [ ] **Step 5: Run unit, build, and Playwright tests**

Run: `npm run test:unit && npm run build && npm run test:e2e`

Expected: PASS including the original 34 mock tests and the new fallback test.

- [ ] **Step 6: Commit**

```powershell
git add apps/desktop/src apps/desktop/e2e
git commit -m "feat: connect desktop shell to native runtime"
```

### Task 5: Prepare The Managed Development CPU Pack

**Files:**
- Create: `scripts/prepare-dev-cpu-pack.ps1`
- Modify: `docs/native-runtime-validation.md`
- Test: PowerShell parser plus script execution

- [ ] **Step 1: Write the script contract and parser check**

Parameters are:

```powershell
param(
  [string]$PackId = 'cpu-dev',
  [string]$DestinationRoot = (Join-Path $env:LOCALAPPDATA 'io.github.soomin-sung-estsoft.local-llm-wiki/runtime-packs'),
  [ValidateSet('Debug','Release')][string]$Configuration = 'Debug'
)
```

Reject Pack IDs outside `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`. Resolve repository-relative build/install directories. Configure `LLW_BACKEND_PACK=CPU`, build, run CTest, install to staging, and require `local_llm_runtime.dll`, `ggml.dll`, `ggml-base.dll`, `ggml-cpu.dll`, `llama.dll`, and `llw_runtime_backend_test.exe`.

- [ ] **Step 2: Implement failure-restoring activation**

Use sibling paths `<PackId>.staging-<pid>` and `<PackId>.backup-<pid>`. Remove only owned staging/backup paths beneath canonical `DestinationRoot`. Rename active to backup, rename staging to active, restore backup on activation failure, and delete backup only after success. Never recursively delete a computed path before verifying it starts with the canonical destination root.

- [ ] **Step 3: Verify parser and execute the script**

Run:

```powershell
$errors = $null; [Management.Automation.Language.Parser]::ParseFile((Resolve-Path 'scripts/prepare-dev-cpu-pack.ps1'), [ref]$null, [ref]$errors) | Out-Null; if ($errors) { $errors | Format-List; exit 1 }
& scripts/prepare-dev-cpu-pack.ps1
```

Expected: parser has zero errors; CMake/CTest pass; the command prints the active `cpu-dev` path containing all six required files.

- [ ] **Step 4: Commit**

```powershell
git add scripts/prepare-dev-cpu-pack.ps1 docs/native-runtime-validation.md
git commit -m "build: prepare managed development CPU pack"
```

### Task 6: Real CPU Integration And Milestone Verification

**Files:**
- Modify only files required by failures reproduced in this task
- Test: existing Rust integration, native backend executable, frontend suites, manual Tauri flow

- [ ] **Step 1: Acquire the pinned tiny GGUF and run native smoke tests**

Run:

```powershell
$model = & scripts/acquire-test-model.ps1
$pack = Join-Path $env:LOCALAPPDATA 'io.github.soomin-sung-estsoft.local-llm-wiki/runtime-packs/cpu-dev'
$env:LLW_TEST_GGUF = $model
$env:LLW_TEST_RUNTIME = Join-Path $pack 'local_llm_runtime.dll'
& (Join-Path $pack 'llw_runtime_backend_test.exe') $model
cargo test -p llm-runtime --test fake_runtime -- --nocapture
```

Expected: actual CPU model load, token, cancellation, and metrics checks pass.

- [ ] **Step 2: Run the complete automated milestone suite**

Run:

```powershell
cargo fmt --all -- --check
cargo test -p llm-runtime --lib
cargo test --workspace --exclude llm-runtime
cargo check -p local-llm-wiki-desktop
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:e2e
git diff --check
```

Expected: every command exits 0; Playwright retains all 34 mock tests plus native fallback coverage.

- [ ] **Step 3: Run the Tauri app and verify the actual user flow**

Run `npm --prefix apps/desktop run tauri -- dev`, select the pinned GGUF, send a Korean prompt, confirm bytes stream correctly, press `Esc`, reload the model, and confirm status metrics update. Capture desktop and minimum-size screenshots and verify no overlap, blank region, or horizontal overflow.

- [ ] **Step 4: Perform one milestone code review**

Review the complete diff once for ownership/lifetime defects, command/event contract drift, path traversal, cancellation races, UTF-8 corruption, mock regressions, and missing cleanup. Reproduce every actionable defect with a failing test before fixing it, then rerun Step 2.

- [ ] **Step 5: Commit final fixes**

```powershell
git add -A
git commit -m "fix: harden CPU native UI milestone"
```

- [ ] **Step 6: Merge and publish**

From the main worktree, fast-forward `feature/cpu-native-ui-bridge` into `main`, push `origin main`, verify `git status --short --branch`, and remove the feature worktree and branch after the pushed commit is confirmed.
