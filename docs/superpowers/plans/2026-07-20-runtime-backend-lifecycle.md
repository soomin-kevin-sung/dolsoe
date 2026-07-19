# Runtime Backend Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bundle a recoverable CPU runtime and provide confirmed, resumable CUDA/Vulkan installation, repair, persistence, and restart activation against one pinned llama.cpp baseline.

**Architecture:** Build scripts produce self-contained stable-ID packs from `llama-baseline.json`. Rust separates source/catalog trust, payload validation, crash-recoverable transactions, CPU bootstrap, inventory, installation, and persisted selection. React renders one lifecycle row per backend and requests confirmation before network or restart actions.

**Tech Stack:** PowerShell, GitHub Actions, CMake/C++, Rust 1.93, Tauri 2, React 19, TypeScript, Vitest, Playwright.

---

## File Map

**Create**

- `native/llm-runtime/llama-baseline.json`: immutable llama.cpp release and official asset digests.
- `apps/desktop/src-tauri/resources/runtime-source.default.json`: relocatable default GitHub Release source.
- `apps/desktop/src-tauri/src/runtime_source.rs`: source override parsing, validation, URL construction, pinned manifest fetch.
- `apps/desktop/src-tauri/src/runtime_transaction.rs`: journaled stable/staging/backup replacement and startup recovery.
- `apps/desktop/src-tauri/src/runtime_bootstrap.rs`: bundled CPU synchronization before worker creation.
- `apps/desktop/src-tauri/src/runtime_selection.rs`: Rust-owned active and one-shot pending backend settings.
- `apps/desktop/src-tauri/src/runtime_host.rs`: optional worker wrapper for ready/recovery startup modes.
- `apps/desktop/src/components/RuntimeBackendRow.tsx`: unified select/install/repair/progress backend row.

**Modify**

- Runtime build/release scripts and workflow: consume baseline, generate internal/catalog manifests, remove signing.
- Rust manifest/archive/download/installer/inventory/path/probe/setup/commands: enforce stable identities and lifecycle.
- Tauri bundle configuration: include CPU ZIP/index and default source.
- TypeScript runtime service/hook/settings/app/styles/tests: separate pack and selection state and add confirmations/restart.
- Existing release and E2E tests: cover generated contracts and lifecycle UI.

## Task 1: Centralize the llama.cpp Baseline and Pack Metadata

**Files:**
- Create: `native/llm-runtime/llama-baseline.json`
- Modify: `native/llm-runtime/CMakeLists.txt`
- Modify: `scripts/build-runtime-release.ps1`
- Modify: `scripts/generate-runtime-manifest.ps1`
- Modify: `scripts/tests/runtime-release.Tests.ps1`

- [ ] **Step 1: Add failing PowerShell assertions**

Assert that the baseline is the only source of tag/commit/ABI/asset values, generated ZIPs use stable backend IDs, and every archive contains `runtime-pack.json` whose payload list excludes itself.

```powershell
$baseline = Get-Content native/llm-runtime/llama-baseline.json -Raw | ConvertFrom-Json
Assert-True ($baseline.releaseTag -eq 'b10068') 'baseline tag mismatch'
Assert-True ($baseline.commit -eq '571d0d540df04f25298d0e159e520d9fc62ed121') 'baseline commit mismatch'
Assert-True ($pack.id -eq $backend.ToLowerInvariant()) 'pack ID must be stable backend ID'
Assert-True ('runtime-pack.json' -notin $pack.files.path) 'manifest must not hash itself'
```

- [ ] **Step 2: Run the focused script test and confirm failure**

Run: `pwsh -NoProfile -File scripts/tests/runtime-release.Tests.ps1`

Expected: FAIL because the baseline/internal pack manifest do not exist and IDs include versions.

- [ ] **Step 3: Add the baseline and consume it**

Define schema `1`, tag `b10068`, the verified commit, ABI `1.1`, platform `windows`, arch `x86_64`, and the existing official CPU/Vulkan/CUDA 12.4 asset names and SHA-256 values. Pass the commit from PowerShell into CMake as `LLW_LLAMA_CPP_COMMIT`; remove repeated constants.

Generate `runtime-pack.json` before ZIP creation with stable `id/backend`, pack version, baseline identity, and every payload file's size/hash. Generate the remote catalog with `assetName`, archive size/hash, and expected identity only.

- [ ] **Step 4: Run the focused script test**

Run: `pwsh -NoProfile -File scripts/tests/runtime-release.Tests.ps1`

Expected: PASS.

## Task 2: Replace Signed Distribution Configuration with a Pinned Source

**Files:**
- Create: `apps/desktop/src-tauri/resources/runtime-source.default.json`
- Create: `apps/desktop/src-tauri/src/runtime_source.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_manifest.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_installer.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`

- [ ] **Step 1: Add failing Rust tests for source and catalog contracts**

Cover whole-file override precedence, malformed override rejection without default fallback, repository/tag/asset traversal rejection, 64-character lowercase SHA-256, GitHub URL composition, bounded catalog fetch, catalog digest mismatch, duplicate backend rejection, and exact ID/backend/platform/arch/release/commit/ABI matching.

```rust
assert_eq!(source.asset_url("cuda.zip").unwrap(),
    "https://github.com/owner/repo/releases/download/runtime-v1/cuda.zip");
assert!(RuntimeSource::parse(br#"{"repository":"../bad"}"#).is_err());
assert!(catalog.validate(&policy).is_err_when_identity_differs());
```

- [ ] **Step 2: Run the focused Rust tests and confirm failure**

Run: `cargo test -p local-llm-wiki-desktop runtime_source runtime_manifest`

Expected: FAIL because `runtime_source` and unsigned catalog contracts do not exist.

- [ ] **Step 3: Implement pinned-source loading**

Add `RuntimeSource { schema_version, provider, repository, release_tag, manifest_asset, manifest_sha256 }`. Load `<app-local-data>/runtime-source.json` when present, otherwise load the bundled default resource. Fetch only the composed GitHub URL, bound it to 4 MiB, and verify raw bytes against `manifest_sha256` before parsing.

Replace `SignedRuntimeManifest` with `RuntimeCatalog`; remove `base64`, `ed25519-dalek`, signature URL/key configuration, and signature fetch paths.

- [ ] **Step 4: Run the focused Rust tests**

Run: `cargo test -p local-llm-wiki-desktop runtime_source runtime_manifest`

Expected: PASS.

## Task 3: Validate Internal Pack Manifests and Digest-Keyed Downloads

**Files:**
- Modify: `apps/desktop/src-tauri/src/runtime_archive.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_download.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_manifest.rs`

- [ ] **Step 1: Add failing archive/download tests**

Add cases for internal/catalog identity mismatch, manifest self-entry, undeclared payload, payload digest mismatch, ABI minor mismatch, mixed backend DLLs, stale partial digest, and incorrect `Content-Range` resume offset.

```rust
assert!(matches!(install_verified_archive(...),
    Err(ArchiveInstallError::IdentityMismatch(_))));
assert_eq!(partial_name("cuda", &digest), format!("cuda-{digest}.zip.part"));
```

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test -p local-llm-wiki-desktop runtime_archive runtime_download`

Expected: FAIL for the new internal-manifest and resume rules.

- [ ] **Step 3: Implement two-layer validation**

Parse `runtime-pack.json` first under the existing ZIP bounds. Require exact catalog identity equality. Exclude the manifest itself from payload declarations, reject undeclared files, and validate every payload size/hash while extracting to staging.

Name partials `<backend>-<archive-sha256>.zip.part`. Accept HTTP `206` only when `Content-Range` starts at the local file length; otherwise restart from byte zero. Delete obsolete partials only inside the validated `.downloads` root.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p local-llm-wiki-desktop runtime_archive runtime_download`

Expected: PASS.

## Task 4: Add Crash-Recoverable Runtime Transactions

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_transaction.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_archive.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing transaction state-machine tests**

Use temporary directories and an injected validator to cover interruption before/after stable-to-backup and staging-to-stable renames, invalid new stable quarantine, valid backup restoration, final validation failure, path escape rejection, and a locked/current pack returning deferred replacement.

```rust
let outcome = recover_transaction(&root, &journal, validate)?;
assert_eq!(outcome, RecoveryOutcome::RestoredBackup);
assert!(root.join("cpu").exists());
```

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test -p local-llm-wiki-desktop runtime_transaction`

Expected: FAIL because the transaction module does not exist.

- [ ] **Step 3: Implement journaled replacement and recovery**

Persist JSON journals under `runtime-packs/.transactions/<backend>.json` using write/flush/rename. Journal exact stable, staging, backup, quarantine names plus archive digest and phase. Validate all paths remain direct descendants of runtime root. On recovery prefer valid stable, then valid staging, then valid backup; quarantine a failed stable before restoration and never choose by timestamp.

Return `Installed` for an unloaded backend and `DeferredUntilRestart` for a currently loaded backend. Do not probe or rename a loaded directory in the app process.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p local-llm-wiki-desktop runtime_transaction`

Expected: PASS.

## Task 5: Bundle and Bootstrap CPU Before Worker Creation

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_bootstrap.rs`
- Create: `apps/desktop/src-tauri/src/runtime_host.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/llm_commands.rs`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `scripts/build-runtime-release.ps1`

- [ ] **Step 1: Add failing bootstrap and host tests**

Cover fresh offline CPU install, matching installed CPU fast path, corrupt CPU replacement, failed replacement retaining a valid old CPU, transaction recovery before resolver creation, and `RecoveryRequired` allowing UI/database startup while inference commands return a focused error.

```rust
assert!(matches!(bootstrap_cpu(...), BootstrapState::Ready { .. }));
assert_eq!(RuntimeHost::recovery("cpu corrupt").status().error,
    Some("cpu corrupt".into()));
```

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test -p local-llm-wiki-desktop runtime_bootstrap runtime_host`

Expected: FAIL because bootstrap/optional host do not exist.

- [ ] **Step 3: Implement bundled CPU resources and degraded startup**

Generate `apps/desktop/src-tauri/resources/runtime-packs/cpu.zip` plus `cpu-index.json` as desktop build inputs and register them under `bundle.resources`. Resolve them through Tauri's resource directory. Recover transactions, synchronize CPU through the journaled pipeline, then create `RuntimeHost::Ready(WorkerHandle)` or `RuntimeHost::RecoveryRequired(error)`.

Change LLM commands to access `RuntimeHost`; recovery mode keeps conversations/UI available but rejects load/submit/metrics calls. The app setup itself must not fail solely because CPU bootstrap failed.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p local-llm-wiki-desktop runtime_bootstrap runtime_host llm_commands`

Expected: PASS.

## Task 6: Enforce Stable Inventory and Persist One-Shot Activation

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_selection.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_packs.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_path.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_install_commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing inventory/selection tests**

Cover only `cpu/cuda/vulkan` being inventoried, CPU-only fallback, unknown directories and `cpu-dev` ignored, integrity/ABI errors as `repair-required`, valid pack with no device as `unavailable`, pending activation consumed once, success promotion, failed activation CPU fallback, and no startup retry loop.

```rust
assert_eq!(inventory.packs.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
    vec!["cpu", "cuda", "vulkan"]);
assert_eq!(selection.consume_pending(failed_probe).active, Backend::Cpu);
assert!(selection.pending_activation.is_none());
```

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test -p local-llm-wiki-desktop runtime_packs runtime_selection runtime_path`

Expected: FAIL under the current free-form IDs, `cpu-dev` fallback, and WebView persistence.

- [ ] **Step 3: Implement lifecycle and selection contracts**

Replace free-form production selection with `RuntimeBackend::{Cpu,Cuda,Vulkan}` mapped to same-name directories. Inventory returns one row per backend with `ready/not-installed/replacement-pending/repair-required/unavailable` and separate selection data.

Persist `activeBackend`, optional `pendingActivation`, attempt marker, and last activation error in app-local JSON using durable replace. Installer writes pending only after complete validation. Startup consumes it once and falls back only to CPU.

Add commands to get selection, request backend activation, and restart after active generation reaches terminal persistence.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p local-llm-wiki-desktop runtime_packs runtime_selection runtime_path`

Expected: PASS.

## Task 7: Integrate GPU Installation, Repair, and Deferred Replacement

**Files:**
- Modify: `apps/desktop/src-tauri/src/runtime_installer.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_install_commands.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_transaction.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Add failing installer integration tests**

Use the existing local HTTP fixture to cover lazy catalog fetch, one active operation, stable backend IDs, source failure isolation, resumable digest-keyed download, immediate unloaded install, active-backend deferred repair, cancellation only before finalization, and pending activation written only after validation.

- [ ] **Step 2: Run focused tests and confirm failure**

Run: `cargo test -p local-llm-wiki-desktop runtime_installer runtime_install_commands`

Expected: FAIL under signed config/versioned IDs/conflict refusal.

- [ ] **Step 3: Wire the new installer pipeline**

Construct installer state from app-local data plus the bundled default source. Fetch catalog only from `list_available_runtime_packs` or explicit install/repair. Restrict network installation to CUDA/Vulkan, emit `downloading/verifying/installing/replacementPending/restartRequired/cancelled/failed`, and preserve active CPU/chat state on every error.

Replace conflict refusal with journaled install or deferred replacement. Record pending activation after success and expose backend/version/size/baseline details for confirmation.

- [ ] **Step 4: Run focused tests**

Run: `cargo test -p local-llm-wiki-desktop runtime_installer runtime_install_commands`

Expected: PASS.

## Task 8: Build the Unified Backend Selection UI

**Files:**
- Create: `apps/desktop/src/components/RuntimeBackendRow.tsx`
- Modify: `apps/desktop/src/services/runtimePacks.ts`
- Modify: `apps/desktop/src/services/runtimePacks.test.ts`
- Modify: `apps/desktop/src/hooks/useRuntimePackInstaller.ts`
- Modify: `apps/desktop/src/hooks/useNativeRuntime.ts`
- Modify: `apps/desktop/src/components/NativeSettingsPanel.tsx`
- Modify: `apps/desktop/src/components/ConfirmDialog.tsx`
- Modify: `apps/desktop/src/NativeApp.tsx`
- Modify: `apps/desktop/src/App.css`

- [ ] **Step 1: Add failing service/component tests**

Cover lazy catalog refresh when settings opens, uninstalled selection opening confirmation without changing desired backend, install confirmation details, progress/cancel, repair action, unavailable explanation, installed result showing restart now/later, pending activation status, and Korean user-facing labels.

```ts
expect(next.desiredBackend).toBe("cpu");
expect(next.confirmInstall?.backend).toBe("cuda");
expect(reduceInstall(installed).restartRequired).toBe(true);
```

- [ ] **Step 2: Run focused frontend tests and confirm failure**

Run: `npm --prefix apps/desktop run test:unit -- runtimePacks`

Expected: FAIL under the disabled segmented control and duplicate install list.

- [ ] **Step 3: Implement unified backend rows and dialogs**

Render CPU/CUDA/Vulkan as fixed rows with icon, status, version/device detail, and contextual action. Selecting `not-installed` or `repair-required` opens confirmation with version, size, baseline, and restart notice. Keep active selection unchanged until install validation succeeds.

On completion show `지금 재시작` and `나중에`. Restart now invokes the orderly generation cancellation/persistence flow and the Tauri restart command. Later preserves current inference and a visible restart-required marker. Do not show a second available-pack list.

- [ ] **Step 4: Run focused frontend tests and build**

Run: `npm --prefix apps/desktop run test:unit -- runtimePacks`

Run: `npm --prefix apps/desktop run build`

Expected: PASS.

## Task 9: Update Runtime Release and Desktop Packaging

**Files:**
- Modify: `.github/workflows/runtime-release.yml`
- Modify: `.github/workflows/ci.yml`
- Modify: `scripts/build-runtime-release.ps1`
- Modify: `scripts/generate-runtime-manifest.ps1`
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Modify: `apps/desktop/src-tauri/build.rs`
- Modify: `scripts/tests/runtime-release.Tests.ps1`

- [ ] **Step 1: Add failing packaging assertions**

Require no signing secret/assets, all three self-contained stable-ID packs from official b10068 release assets, a catalog containing relative asset names, and desktop resources containing the CPU ZIP/index plus default source.

- [ ] **Step 2: Run focused release checks and confirm failure**

Run: `pwsh -NoProfile -File scripts/tests/runtime-release.Tests.ps1`

Expected: FAIL while workflow still requires signing and desktop resources are absent.

- [ ] **Step 3: Update workflow and build integration**

Remove the signing environment and `.sig`/public-key release assets. Generate and publish ZIPs plus pinned catalog. Make desktop build preparation create/copy CPU resources deterministically from the same baseline and fail clearly when a release bundle is requested without them. Keep development able to use a generated local CPU resource.

- [ ] **Step 4: Run focused release checks**

Run: `pwsh -NoProfile -File scripts/tests/runtime-release.Tests.ps1`

Expected: PASS.

## Task 10: Final Review and One Full Verification Pass

**Files:**
- Modify only files required by concrete final-review findings.
- Modify: `apps/desktop/e2e/ui-states.spec.ts` if lifecycle coverage is missing.

- [ ] **Step 1: Perform one focused code review**

Review trust boundaries, Windows journal recovery, CPU startup ordering, one-shot pending activation, GPU failure isolation, and user confirmation. Fix correctness findings only; do not start a stylistic re-review loop.

- [ ] **Step 2: Run the complete Rust verification once**

Run: `cargo fmt --all -- --check`

Run: `cargo clippy --workspace --all-targets -- -D warnings`

Run: `cargo test --workspace`

Expected: all PASS.

- [ ] **Step 3: Run the complete frontend verification once**

Run: `npm --prefix apps/desktop run test:unit`

Run: `npm --prefix apps/desktop run build`

Run: `npm --prefix apps/desktop run test:e2e`

Expected: all PASS.

- [ ] **Step 4: Run release and packaging smoke checks once**

Run: `pwsh -NoProfile -File scripts/tests/runtime-release.Tests.ps1`

Run a CPU source-pack build into a temporary output root and inspect ZIP/index/catalog contents. Do not download/build CUDA or Vulkan again if their focused manifest fixture checks already passed.

- [ ] **Step 5: Inspect final scope**

Run: `git diff --check`

Run: `git status --short`

Expected: no whitespace errors; `.runtime-release-test/` and `.runtime-release-vulkan-test/` remain untracked and untouched unless the user separately asks to remove them.
