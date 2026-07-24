# Installed Runtime Pack Selection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect trusted installed CPU/CUDA/Vulkan runtime packs, let the user select an available backend, and safely reload the current GGUF model through that pack while preserving the selection across app restarts.

**Architecture:** A Rust `runtime_packs` module scans and probes managed pack directories and exposes one inventory command. The model load request becomes backend-aware. TypeScript owns the small WebView preference and pending/applied selection state, while `useConversationWorkspace` waits for terminal persistence before runtime changes.

**Tech Stack:** Rust, Tauri 2, `llm-runtime`, React 19, TypeScript, Vitest, Playwright, WebView `localStorage`.

---

## File Map

- Create `apps/desktop/src-tauri/src/runtime_packs.rs`: trusted-root inventory, DTOs, backend normalization, fallback, Tauri command.
- Modify `apps/desktop/src-tauri/src/lib.rs`: register inventory command.
- Modify `apps/desktop/src-tauri/src/llm_dto.rs`: backend-aware load DTO.
- Modify `apps/desktop/src-tauri/src/llm_worker.rs`: backend/device/GPU offload model options.
- Create `apps/desktop/src/services/runtimePacks.ts`: inventory client, preference storage, deterministic selection and transition helpers.
- Create `apps/desktop/src/services/runtimePacks.test.ts`: command, preference, fallback, and transition tests.
- Modify `apps/desktop/src/services/nativeRuntime.ts`: backend-aware request contract.
- Modify `apps/desktop/src/services/nativeRuntime.test.ts`: command argument contract.
- Modify `apps/desktop/src/hooks/useNativeRuntime.ts`: inventory and applied/pending state.
- Modify `apps/desktop/src/hooks/useConversationWorkspace.ts`: cancel and persist before applying runtime.
- Modify `apps/desktop/src/components/NativeSettingsPanel.tsx`: inventory-driven segmented control.
- Modify `apps/desktop/src/components/SettingsPanel.tsx`: matching mock interaction for browser E2E.
- Modify `apps/desktop/src/components/NativeDiagnosticsView.tsx`: selected pack version, commit, and ABI rows.
- Modify `apps/desktop/src/NativeApp.tsx`: settings props and reload action.
- Modify `apps/desktop/src/MockApp.tsx`, `apps/desktop/src/services/runtime.ts`: testable mock runtime choices.
- Modify `apps/desktop/src/App.css`: compact validation/error metadata.
- Modify `apps/desktop/e2e/ui-states.spec.ts`: installed/missing runtime expectations.
- Modify `docs/native-runtime-validation.md`: native smoke procedure.

### Task 1: Trusted runtime pack inventory

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_packs.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing inventory tests**

Create tests in `runtime_packs.rs` using a temporary trusted root and an injected probe closure:

```rust
#[test]
fn inventory_keeps_invalid_packs_without_failing_ready_packs() {
    let root = TestDir::new();
    create_pack(root.path(), "broken");
    create_pack(root.path(), "cpu-dev");
    let inventory = scan_runtime_packs(root.path(), |id, _| match id {
        "cpu-dev" => Ok(probed_cpu("CPU 0")),
        _ => Err("ABI mismatch".into()),
    }).unwrap();
    assert_eq!(inventory.packs[0].status, RuntimePackStatus::Invalid);
    assert_eq!(inventory.packs[1].backend, Some(RuntimeBackend::Cpu));
}

#[test]
fn fallback_prefers_cpu_dev_then_other_cpu_then_any_ready_pack() {
    let packs = vec![ready("vulkan-a", Vulkan), ready("cpu-z", Cpu), ready("cpu-dev", Cpu)];
    assert_eq!(select_fallback(&packs).unwrap().id, "cpu-dev");
}
```

- [ ] **Step 2: Run focused tests and verify RED**

Run `cargo test -p dolsoe-desktop runtime_packs::tests -- --nocapture`.

Expected: FAIL because `runtime_packs` and inventory types do not exist.

- [ ] **Step 3: Implement inventory contracts**

Implement these exact serialized types:

```rust
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeBackend { Cpu, Cuda, Vulkan }

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimePackStatus { Ready, Invalid }

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeDeviceDto { pub index: u32, pub id: String, pub name: String, pub vendor: String }

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePackDto {
    pub id: String,
    pub backend: Option<RuntimeBackend>,
    pub status: RuntimePackStatus,
    pub runtime_version: Option<String>,
    pub llama_cpp_commit: Option<String>,
    pub abi_major: Option<u32>,
    pub abi_minor: Option<u32>,
    pub devices: Vec<RuntimeDeviceDto>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RuntimePackInventoryDto { pub packs: Vec<RuntimePackDto>, pub fallback_pack_id: Option<String> }
```

Scan immediate child directories, validate each ID with `validate_runtime_pack_id`, resolve through `RuntimePackResolver`, and probe `RuntimeLibrary`. Choose the primary capability in CUDA, Vulkan, CPU order, enumerate that backend's devices, and mark no-device packs invalid. Sort by ID. Fallback order is ready `cpu-dev`, first ready CPU by ID, then first ready pack by ID.

- [ ] **Step 4: Add and register the command**

Add `list_runtime_packs(app: tauri::AppHandle) -> Result<RuntimePackInventoryDto, String>` using `spawn_blocking`, then register `mod runtime_packs` and the command in `lib.rs`.

- [ ] **Step 5: Verify GREEN and commit**

```powershell
cargo test -p dolsoe-desktop runtime_packs::tests -- --nocapture
cargo check -p dolsoe-desktop
git add apps/desktop/src-tauri/src/runtime_packs.rs apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: inventory installed runtime packs"
```

### Task 2: Backend-aware model loading

**Files:**
- Modify: `apps/desktop/src-tauri/src/llm_dto.rs`
- Modify: `apps/desktop/src-tauri/src/llm_worker.rs`

- [ ] **Step 1: Write failing backend tests**

```rust
#[test]
fn model_target_maps_backend_and_gpu_layers() {
    let (cpu, cpu_index, cpu_layers) = model_target("cpu", 0).unwrap();
    let (cuda, cuda_index, cuda_layers) = model_target("cuda", 2).unwrap();
    assert!(matches!(cpu, Backend::Cpu));
    assert_eq!((cpu_index, cpu_layers), (0, 0));
    assert!(matches!(cuda, Backend::Cuda));
    assert_eq!((cuda_index, cuda_layers), (2, -1));
}

#[test]
fn model_target_rejects_unknown_backend() {
    assert!(model_target("metal", 0).is_err());
}
```

- [ ] **Step 2: Run `cargo test -p dolsoe-desktop model_target -- --nocapture`**

Expected: FAIL because `model_target` and request fields do not exist.

- [ ] **Step 3: Extend the request and worker**

Add `backend: String` and `device_index: u32` to `LoadModelRequest`. Implement:

```rust
fn model_target(value: &str, device_index: u32) -> WorkerResult<(Backend, u32, i32)> {
    match value {
        "cpu" => Ok((Backend::Cpu, device_index, 0)),
        "cuda" => Ok((Backend::Cuda, device_index, -1)),
        "vulkan" => Ok((Backend::Vulkan, device_index, -1)),
        _ => Err("backend must be cpu, cuda, or vulkan".into()),
    }
}
```

Pass all three values to `ModelOptions` and report uppercase backend in status.

- [ ] **Step 4: Verify and commit**

```powershell
cargo test -p dolsoe-desktop --lib
cargo test -p llm-runtime --lib
git add apps/desktop/src-tauri/src/llm_dto.rs apps/desktop/src-tauri/src/llm_worker.rs
git commit -m "feat: load models on selected backends"
```

### Task 3: Frontend inventory and preference service

**Files:**
- Create: `apps/desktop/src/services/runtimePacks.ts`
- Create: `apps/desktop/src/services/runtimePacks.test.ts`
- Modify: `apps/desktop/src/services/nativeRuntime.ts`
- Modify: `apps/desktop/src/services/nativeRuntime.test.ts`

- [ ] **Step 1: Write failing service tests**

```typescript
it("lists installed packs through the fixed command", async () => {
  await new RuntimePackService(bindings).list();
  expect(invoke).toHaveBeenCalledWith("list_runtime_packs");
});

it("rejects a stored preference that is not ready", () => {
  const inventory = fixtureInventory([invalidPack("cuda-broken"), readyCpu("cpu-dev")]);
  expect(resolveRuntimeSelection(inventory, { packId: "cuda-broken", backend: "cuda" }))
    .toEqual({ packId: "cpu-dev", backend: "cpu", deviceIndex: 0 });
});

it("leaves an invalid stored preference intact while using fallback", () => {
  storage.setItem(PREFERENCE_KEY, JSON.stringify({ packId: "missing", backend: "cuda" }));
  resolveRuntimeSelection(inventory, readRuntimePreference(storage));
  expect(storage.getItem(PREFERENCE_KEY)).toContain("missing");
});
```

- [ ] **Step 2: Run `npm --prefix apps/desktop run test:unit -- runtimePacks`**

Expected: FAIL because `runtimePacks.ts` does not exist.

- [ ] **Step 3: Implement service and selection helpers**

Define `RuntimeBackend`, `RuntimeDevice`, `RuntimePack`, `RuntimePackInventory`, and:

```typescript
export interface RuntimeSelection { packId: string; backend: RuntimeBackend; deviceIndex: number; }
export const PREFERENCE_KEY = "dolsoe.runtime-pack";
export function readRuntimePreference(storage: Storage): RuntimePreference | null;
export function writeRuntimePreference(storage: Storage, value: RuntimeSelection): void;
export function resolveRuntimeSelection(inventory: RuntimePackInventory, preference: RuntimePreference | null): RuntimeSelection | null;
```

Accept preferences only when ID, backend, ready status, and a device match. Malformed JSON returns null. Persist only pack ID and backend. Add `list_runtime_packs` binding and extend frontend `LoadModelRequest` with backend and device index.

- [ ] **Step 4: Verify and commit**

```powershell
npm --prefix apps/desktop run test:unit -- runtimePacks nativeRuntime
npm --prefix apps/desktop run build
git add apps/desktop/src/services/runtimePacks.ts apps/desktop/src/services/runtimePacks.test.ts apps/desktop/src/services/nativeRuntime.ts apps/desktop/src/services/nativeRuntime.test.ts
git commit -m "feat: add runtime pack frontend service"
```

### Task 4: Safe runtime transition state

**Files:**
- Modify: `apps/desktop/src/services/runtimePacks.ts`
- Modify: `apps/desktop/src/services/runtimePacks.test.ts`
- Modify: `apps/desktop/src/hooks/useNativeRuntime.ts`
- Modify: `apps/desktop/src/hooks/useConversationWorkspace.ts`

- [ ] **Step 1: Write failing transition-order tests**

```typescript
it("unloads, saves, and reloads the current model in order", async () => {
  const calls: string[] = [];
  await applyRuntimeSelection(next, {
    modelPath: "D:\\models\\tiny.gguf",
    unload: async () => { calls.push("unload"); },
    persist: () => { calls.push("persist"); },
    load: async () => { calls.push("load"); },
  });
  expect(calls).toEqual(["unload", "persist", "load"]);
});
```

Add a no-model case that expects `persist` only.

- [ ] **Step 2: Run the test and verify RED**

Run `npm --prefix apps/desktop run test:unit -- runtimePacks`.

Expected: FAIL because `applyRuntimeSelection` does not exist.

- [ ] **Step 3: Implement hook state**

Bootstrap inventory and preference on mount. Expose `runtimePacks`, `runtimePackError`, `appliedRuntime`, `pendingRuntime`, `setPendingBackend`, and `applyPendingRuntime`. Replace hardcoded `cpu-dev` in `loadPath` with the applied selection. Pending backend resolves to the first ready pack and first device for that backend.

Use the tested transition helper to unload, persist, and conditionally reload. Keep the new preference when reload fails, and surface the load error through existing native state.

- [ ] **Step 4: Serialize active generation cancellation**

Wrap apply in `useConversationWorkspace`:

```typescript
const applyPendingRuntime = useCallback(async () => {
  const active = stateRef.current.activeTurn;
  if (active) await cancelSource(active.conversationId);
  await runtime.applyPendingRuntime();
}, [cancelSource, runtime]);
```

- [ ] **Step 5: Verify and commit**

```powershell
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
git add apps/desktop/src/services/runtimePacks.ts apps/desktop/src/services/runtimePacks.test.ts apps/desktop/src/hooks/useNativeRuntime.ts apps/desktop/src/hooks/useConversationWorkspace.ts
git commit -m "feat: manage runtime pack selection state"
```

### Task 5: Settings UI integration

**Files:**
- Modify: `apps/desktop/src/components/NativeSettingsPanel.tsx`
- Modify: `apps/desktop/src/components/SettingsPanel.tsx`
- Modify: `apps/desktop/src/components/NativeDiagnosticsView.tsx`
- Modify: `apps/desktop/src/NativeApp.tsx`
- Modify: `apps/desktop/src/MockApp.tsx`
- Modify: `apps/desktop/src/services/runtime.ts`
- Modify: `apps/desktop/src/App.css`
- Modify: `apps/desktop/e2e/ui-states.spec.ts`

- [ ] **Step 1: Write failing E2E tests**

```typescript
test("runtime control disables unavailable backends", async ({ page }) => {
  await page.goto("/?state=settings");
  await expect(page.getByRole("button", { name: "Vulkan" })).toBeDisabled();
  await expect(page.getByText("Vulkan 런타임 팩이 설치되지 않았습니다")).toBeVisible();
});

test("runtime selection exposes pending reload state", async ({ page }) => {
  await page.goto("/?state=settings");
  await page.getByRole("button", { name: "CUDA" }).click();
  await expect(page.getByText("재로드")).toBeVisible();
  await expect(page.getByRole("button", { name: "적용하고 모델 다시 로드" })).toBeVisible();
});
```

- [ ] **Step 2: Run `npm --prefix apps/desktop run test:e2e -- --grep "runtime"`**

Expected: FAIL because the settings control is CPU-only.

- [ ] **Step 3: Connect inventory controls**

Add inventory, applied/pending selection, change/apply callbacks, and runtime error props to `NativeSettingsPanel`. Build CPU/CUDA/Vulkan items from ready packs. Show pending pack ID and first device below the segment. Show missing/invalid explanations using existing 11px metadata styling. The reload badge and footer depend on pending versus applied state.

Wire native props in `NativeApp`. Pass the applied pack to `NativeDiagnosticsView` and add runtime pack ID, runtime version, llama.cpp commit, and ABI rows with `—` fallbacks. Extend `SettingsPanel`, mock snapshots, and local mock pending state so Playwright can exercise CPU/CUDA selection without Tauri. Do not add installation controls.

- [ ] **Step 4: Verify and commit**

```powershell
npm --prefix apps/desktop run test:e2e -- --grep "runtime"
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
git add apps/desktop/src/components/NativeSettingsPanel.tsx apps/desktop/src/components/SettingsPanel.tsx apps/desktop/src/components/NativeDiagnosticsView.tsx apps/desktop/src/NativeApp.tsx apps/desktop/src/MockApp.tsx apps/desktop/src/services/runtime.ts apps/desktop/src/App.css apps/desktop/e2e/ui-states.spec.ts
git commit -m "feat: add runtime backend selection UI"
```

### Task 6: Native CPU smoke and final verification

**Files:**
- Modify: `docs/native-runtime-validation.md`

- [ ] **Step 1: Prepare managed fixtures**

```powershell
& scripts/prepare-dev-cpu-pack.ps1
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
```

- [ ] **Step 2: Run full automated verification**

```powershell
cargo fmt --all -- --check
cargo test -p llm-runtime --lib
cargo test --workspace --exclude llm-runtime
$env:LLW_TEST_RUNTIME = Join-Path $env:LOCALAPPDATA 'ai.dolsoe.desktop\runtime-packs\cpu-dev\local_llm_runtime.dll'
cargo test -p llm-runtime --test fake_runtime -- --nocapture
cargo check -p dolsoe-desktop
npm --prefix apps/desktop run test:unit
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:e2e
git diff --check
```

- [ ] **Step 3: Run the actual Tauri smoke**

Run `npm --prefix apps/desktop run tauri -- dev`, then verify:

1. Settings lists `cpu-dev` and the host CPU device.
2. CUDA/Vulkan are disabled without corresponding ready packs.
3. The tiny GGUF loads through resolved CPU selection.
4. Restart restores the CPU preference.
5. Applying runtime reload during generation records a terminal cancelled message before reload.

- [ ] **Step 4: Document and commit**

Add the inventory/selection smoke procedure and observed result to `docs/native-runtime-validation.md`, then commit with `docs: validate installed runtime selection`.

- [ ] **Step 5: Review and integrate**

Review `main..HEAD` once for runtime lifetime, trusted paths, preference validation, cancellation ordering, and hardware gates. Fix Critical/Important findings, rerun full verification, fast-forward merge to `main`, verify the merged result, push `origin main`, and remove the feature worktree and branch.
