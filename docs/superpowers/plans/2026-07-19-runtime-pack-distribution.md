# Runtime Pack Distribution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish compatible Windows runtime packs and let the Tauri app securely download, verify, and install the latest pack for each backend.

**Architecture:** A small Rust installer module validates a signed release manifest, downloads one archive into the managed runtime root, verifies every declared file, and atomically promotes a staging directory. Tauri commands expose discovery, install, cancel, and progress events; the React settings panel consumes those commands without coupling chat availability to network state. A dedicated GitHub Actions workflow builds the three backend packs and creates the signed release assets.

**Tech Stack:** Rust 1.93, Tauri 2, reqwest, ed25519-dalek, sha2, zip, semver, React 19, Vitest, Playwright, PowerShell, GitHub Actions

---

### Task 1: Runtime release manifest contract

**Files:**
- Create: `packages/runtime-manifests/schema/runtime-manifest.schema.json`
- Create: `apps/desktop/src-tauri/src/runtime_manifest.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Test: `apps/desktop/src-tauri/src/runtime_manifest.rs`

- [ ] Add failing Rust tests for valid manifest parsing, unsupported schema, app version mismatch, ABI mismatch, duplicate backend, invalid pack ID, non-GitHub HTTPS asset URL, malformed SHA-256, and Ed25519 signature rejection.
- [ ] Run `cargo test -p dolsoe-desktop runtime_manifest::tests -- --nocapture` and confirm failures are caused by missing manifest types and validation.
- [ ] Add `base64`, `ed25519-dalek`, `semver`, and `sha2` dependencies and implement `SignedRuntimeManifest::verify_and_parse(raw, signature, public_key, policy)` over the exact downloaded bytes.
- [ ] Add the JSON schema matching the Rust camelCase contract and validate representative fixtures in tests.
- [ ] Re-run the focused tests and commit `feat: define signed runtime manifest contract`.

### Task 2: Safe archive verification and atomic installation

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_archive.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Test: `apps/desktop/src-tauri/src/runtime_archive.rs`

- [ ] Add failing tests that build ZIP fixtures for a valid pack, `../` traversal, absolute paths, duplicate entries, symlink entries, undeclared files, missing files, size mismatch, checksum mismatch, wrong backend DLL, existing identical pack, and existing conflicting pack.
- [ ] Run `cargo test -p dolsoe-desktop runtime_archive::tests -- --nocapture` and verify the expected failures.
- [ ] Add `zip` and `hex` dependencies and implement bounded extraction with `enclosed_name`, normalized relative paths, duplicate rejection, file count and total-size limits, per-file hash validation, backend-specific required file validation, owned staging cleanup, and final directory rename.
- [ ] Re-run focused tests and commit `feat: install verified runtime archives atomically`.

### Task 3: Download engine and install state machine

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_download.rs`
- Create: `apps/desktop/src-tauri/src/runtime_installer.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Test: `apps/desktop/src-tauri/src/runtime_download.rs`
- Test: `apps/desktop/src-tauri/src/runtime_installer.rs`

- [ ] Add failing tests using a local HTTP fixture server for full download, Range resume, server ignoring Range, declared-size overflow, checksum mismatch, cancellation, single-active-install enforcement, and terminal state cleanup.
- [ ] Run focused installer tests and confirm failures occur before implementation.
- [ ] Add `reqwest` with rustls/stream and `tokio-util`; implement chunked download to `.downloads/<pack-id>.zip.part`, Range resume, progress callbacks, cooperative cancellation, archive checksum verification, and the `downloading -> verifying -> installing -> terminal` state machine.
- [ ] Keep manifest URL and public key in `RuntimeDistributionConfig`; production values come from compile-time `LLW_RUNTIME_MANIFEST_URL` and `LLW_RUNTIME_MANIFEST_PUBLIC_KEY`, while tests inject local values.
- [ ] Re-run focused tests and commit `feat: download runtime packs with progress`.

### Task 4: Tauri runtime installation commands

**Files:**
- Create: `apps/desktop/src-tauri/src/runtime_install_commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/runtime_packs.rs`
- Test: `apps/desktop/src-tauri/src/runtime_install_commands.rs`

- [ ] Add failing command-layer tests for available-pack DTO mapping, installed-state mapping, install start, unknown pack rejection, busy rejection, cancellation, and progress DTO camelCase serialization.
- [ ] Run focused command tests and confirm expected failures.
- [ ] Manage one `RuntimeInstaller` in Tauri state, register `list_available_runtime_packs`, `install_runtime_pack`, and `cancel_runtime_pack_install`, and emit `runtime-pack-install-progress` from the installer callback.
- [ ] Ensure remote lookup errors do not change installed inventory or LLM worker state.
- [ ] Re-run desktop Rust tests and commit `feat: expose runtime pack installation commands`.

### Task 5: Frontend installation state and settings UI

**Files:**
- Modify: `apps/desktop/src/services/runtimePacks.ts`
- Modify: `apps/desktop/src/services/runtimePacks.test.ts`
- Create: `apps/desktop/src/hooks/useRuntimePackInstaller.ts`
- Create: `apps/desktop/src/hooks/useRuntimePackInstaller.test.ts`
- Modify: `apps/desktop/src/components/NativeSettingsPanel.tsx`
- Modify: `apps/desktop/src/NativeApp.tsx`
- Modify: `apps/desktop/src/styles.css`

- [ ] Add failing Vitest coverage for list/install/cancel bindings and install event transitions through downloading, verifying, installing, installed, cancelled, and failed.
- [ ] Run `npm --prefix apps/desktop run test:unit -- runtimePacks useRuntimePackInstaller` and confirm expected failures.
- [ ] Extend the service types and implement the hook with one active installation, event cleanup, refresh-on-installed, and network-error isolation.
- [ ] Connect the existing settings design to real pack rows showing backend, version, size, install/cancel controls, progress, failure, and restart-required completion.
- [ ] Re-run frontend unit tests and commit `feat: install runtime packs from settings`.

### Task 6: UI state and end-to-end coverage

**Files:**
- Modify: `apps/desktop/src/services/runtime.ts`
- Modify: `apps/desktop/src/services/mockRuntime.ts`
- Modify: `apps/desktop/src/components/PackRow.tsx`
- Modify: `apps/desktop/e2e/ui-states.spec.ts`

- [ ] Add failing Playwright expectations for an available CUDA pack, download progress, disabled competing installs, failure recovery, and installed/restart-required state.
- [ ] Run the focused Playwright tests and verify they fail for missing states.
- [ ] Align mock state and shared pack-row presentation with the native UI while retaining the approved visual design and responsive dimensions.
- [ ] Re-run Playwright and production frontend build, then commit `test: cover runtime pack installation states`.

### Task 7: Runtime release asset builder

**Files:**
- Create: `scripts/build-runtime-release.ps1`
- Create: `scripts/generate-runtime-manifest.ps1`
- Create: `scripts/tests/runtime-release.Tests.ps1`
- Create: `THIRD_PARTY_NOTICES.txt`
- Modify: `docs/native-runtime-validation.md`

- [ ] Add failing Pester-independent PowerShell fixture checks for deterministic asset names, required DLLs, forbidden mixed backend DLLs, file hashes, manifest fields, and missing signing key failure.
- [ ] Run the script fixture mode and confirm failure before the builder exists.
- [ ] Implement backend build/stage/ZIP creation and manifest generation; sign raw manifest bytes with a PKCS#8 Ed25519 key supplied by `LLW_RUNTIME_SIGNING_KEY` and emit the detached base64 signature.
- [ ] Document local unsigned fixture generation separately from publishable signed release generation; production assets must never accept unsigned manifests.
- [ ] Run script tests and commit `build: create signed runtime release assets`.

### Task 8: GitHub Release workflow

**Files:**
- Create: `.github/workflows/runtime-release.yml`
- Modify: `.github/workflows/ci.yml`
- Test: `scripts/tests/runtime-release.Tests.ps1`

- [ ] Extend static workflow tests to require pinned Windows toolchains, CPU/Vulkan hosted jobs, gated CUDA self-hosted job, artifact transfer, signature secret, and non-overwriting release upload.
- [ ] Run static tests and confirm failure for the missing workflow.
- [ ] Add tag/manual triggers, backend build jobs, a manifest/signing job, and `gh release upload` with explicit asset names and least-privilege `contents: write` only on the publish job.
- [ ] Add CI execution of manifest/archive unit tests without publishing.
- [ ] Re-run script/static tests and commit `ci: publish signed runtime packs`.

### Task 9: Full verification and native smoke

**Files:**
- Modify: `docs/native-runtime-validation.md`

- [ ] Run `cargo fmt --all --check`, desktop Rust tests, workspace Rust tests, native CTest, frontend unit tests, Playwright, and production build.
- [ ] Generate a local signed manifest and serve it from a local HTTP fixture; install a CPU release pack through the Tauri command and verify restart discovery.
- [ ] If CUDA Toolkit is available, build/install the CUDA release pack and validate the RTX 3070 path; otherwise record the exact hardware-gated prerequisite without claiming CUDA runtime success.
- [ ] Review changed files for secret material, private keys, test URLs, placeholder values, mixed backend DLLs, and unbounded archive/network inputs.
- [ ] Update validation results and commit `docs: validate runtime pack installation`.
