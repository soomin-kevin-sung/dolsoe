# Runtime Backend Lifecycle Design

## 1. Goal

Provide a reliable Windows 11 x64 lifecycle for the `cpu`, `cuda`, and `vulkan`
llama.cpp runtime backends. CPU must work offline immediately after installing the
desktop app. CUDA and Vulkan are installed on demand from a movable GitHub
repository, and a failed install or repair must never break the current CPU
runtime or active conversation data.

macOS and Metal remain future work. The contracts in this design keep platform
and architecture fields so that those packs can be added without changing the
Windows pack identity model.

## 2. Approaches Considered

### Chosen: self-contained packs with one pinned baseline

Each backend has a complete, isolated directory containing the bridge, llama.cpp,
GGML, and backend-specific dependencies. CPU is bundled with the app; CUDA and
Vulkan are fetched on demand. All three are built from one pinned llama.cpp
release and bridge ABI.

This uses more disk space than shared DLLs, but prevents DLL search-order bugs and
mixed-release installations. It also makes validation, repair, and later macOS
packaging straightforward.

### Rejected: shared core DLLs plus backend overlays

This saves disk space, but an update can mix a bridge from one release with GGML
or a backend DLL from another. Atomic replacement and rollback also become more
complex because several installed backends share mutable files.

### Rejected for now: run each backend through a llama-server sidecar

Process isolation makes backend switching and cancellation simpler, but adds a
second protocol and process lifecycle while the existing C ABI worker already
supports the required local inference path. It can be reconsidered if future
backend DLL unloading proves unreliable in practice.

## 3. Stable Identity and Storage

The only supported Windows pack IDs are `cpu`, `cuda`, and `vulkan`. Version or
distribution labels such as `cpu-dev`, `cpu-bundled`, or `cuda-2026.07.1` are not
part of an ID.

Installed packs live below Tauri's application local-data directory:

```text
runtime-packs/
  cpu/
    runtime-pack.json
    local_llm_runtime.dll
    llama.dll
    ggml.dll
    ggml-base.dll
    ggml-cpu*.dll
  cuda/
    runtime-pack.json
    ...the complete CPU-capable base...
    ggml-cuda.dll
    CUDA redistributable DLLs
  vulkan/
    runtime-pack.json
    ...the complete CPU-capable base...
    ggml-vulkan.dll
```

Every directory is self-contained and DLLs are never shared between packs. Old
development directories such as `cpu-dev` are neither selected nor deleted
automatically. This avoids deleting developer or user files while removing them
from production fallback behavior.

Pack IDs stay stable across updates. Version, release, commit, and digest data are
metadata inside the manifests, not directory names.

## 4. Build-Time Baseline

`native/llm-runtime/llama-baseline.json` is the build-time source of truth. It is
not copied unchanged into an installation. It records:

- schema version;
- llama.cpp release tag and immutable commit;
- bridge ABI major and minor;
- Windows architecture;
- official CPU, CUDA, and Vulkan release asset names and SHA-256 digests;
- CUDA redistributable asset/version when applicable.

The initial baseline is llama.cpp tag `b10068`, commit
`571d0d540df04f25298d0e159e520d9fc62ed121`. The build scripts must consume this
file rather than repeat the tag or commit as constants.

Release generation builds the custom bridge once per backend against that exact
baseline, obtains only official assets declared by the baseline, and rejects a
pack whose bridge probe reports a different commit or ABI.

Each ZIP contains `runtime-pack.json`. It records:

```json
{
  "schemaVersion": 1,
  "id": "cuda",
  "backend": "cuda",
  "packVersion": "2026.07.1",
  "platform": "windows",
  "arch": "x86_64",
  "llamaCppRelease": "b10068",
  "llamaCppCommit": "571d0d540df04f25298d0e159e520d9fc62ed121",
  "abiMajor": 1,
  "abiMinor": 1,
  "files": [
    { "path": "local_llm_runtime.dll", "size": 123, "sha256": "..." }
  ]
}
```

No `.runtime-build` marker is needed. Existing CMake output, release staging, and
the generated Tauri resource directory are sufficient build artifacts.

## 5. Distribution Source and Repository Moves

The app includes `runtime-source.default.json`. A user or developer may replace
the complete source definition with:

```text
<app-local-data>/runtime-source.json
```

The override is loaded as a whole; fields are not merged with defaults. Its
schema is:

```json
{
  "schemaVersion": 1,
  "provider": "github-release",
  "repository": "owner/repository",
  "releaseTag": "runtime-v2026.07.1",
  "manifestAsset": "runtime-manifest.json",
  "manifestSha256": "64 lowercase hexadecimal characters"
}
```

Only HTTPS GitHub Release downloads are supported in this milestone. Repository,
tag, asset-name, and digest values are strictly validated. Arbitrary hosts and
local-folder repositories are excluded by YAGNI.

The remote `runtime-manifest.json` uses relative `assetName` values, not absolute
URLs. The app combines `repository`, `releaseTag`, and `assetName`, so moving the
repository only requires changing the source file. The catalog owns only archive
location, archive size and SHA-256, and the expected pack identity: ID, backend,
pack version, platform, architecture, llama.cpp release and commit, and bridge
ABI. The internal `runtime-pack.json` owns the payload file list and per-file
digests. It excludes itself from that list; the containing archive digest protects
it. Installation requires all identity fields in both layers to match exactly.

There is no signing key. Integrity is established by this pinned chain:

```text
runtime-source manifestSha256
  -> downloaded runtime-manifest.json
  -> selected archive sha256
  -> runtime-pack.json and per-file sha256
  -> live bridge ABI, commit, backend, and device probe
```

Changing the runtime catalog intentionally therefore requires an app update or a
local source override with the new manifest digest. A compromised release alone
cannot replace a pinned manifest or pack unnoticed. The live commit probe proves
the bridge's declared build identity, not the provenance of `llama.dll`; actual
llama.cpp provenance is guaranteed by verifying official baseline asset hashes at
build time and carrying the generated payload and archive hashes through this
chain. At runtime, ID must equal backend and release, commit, platform,
architecture, ABI major, and ABI minor must exactly match the supported baseline.

## 6. CPU Bundling and Startup

The desktop bundle contains a generated `cpu.zip` and a small bundled index with
the archive size, SHA-256, pack version, baseline commit, and ABI. The Tauri
resource configuration embeds both files.

Startup order is fixed:

1. Resolve and create the application local-data and runtime-pack directories.
2. Recover any interrupted runtime-pack transaction from its journal.
3. Synchronize bundled CPU into `runtime-packs/cpu`.
4. Probe the resulting CPU pack with a short-lived helper process.
5. Construct the runtime resolver and optional runtime host.
6. Open the normal inventory and user interface.

If installed CPU metadata and the bundled index match, startup performs the live
bridge probe but does not hash every DLL. If CPU is absent, stale, or invalid, the
app extracts the bundled archive to a staging directory and performs full archive,
manifest, per-file, and bridge validation.

CPU replacement is crash-recoverable:

1. fully prepare and validate staging with the short-lived probe helper;
2. write and durably flush a transaction journal naming backend, staging, stable,
   backup, expected digest, and current phase;
3. rename an existing `cpu` directory to a uniquely named backup and advance the
   journal;
4. rename staging to `cpu` and advance the journal;
5. probe the final path in another short-lived process;
6. delete the backup and journal only after success;
7. on failure, first quarantine the failed new stable directory, then restore the
   validated backup and remove the journal.

Before any worker exists, startup replays an incomplete journal by validating the
stable, staging, and backup candidates against the journal digest. It keeps a
valid stable candidate, otherwise promotes a valid staging candidate, otherwise
restores a valid backup. It never chooses by timestamp alone. Stale staging,
backup, quarantine, and journal files are cleaned only after their paths and
naming patterns are validated.

The probe helper is a separate executable/process so its DLL handles terminate
before any directory rename. A valid existing CPU remains usable if
synchronization fails. If no valid CPU can be recovered, bootstrap returns
`RecoveryRequired` rather than a Tauri setup error. The conversation database and
UI still initialize, while an optional runtime host rejects inference commands
with a focused CPU recovery error. `Ready` is the only bootstrap result that
creates the normal worker.

## 7. GPU Catalog, Install, and Repair

The remote catalog is loaded lazily when the settings panel opens or the user
selects an uninstalled GPU backend. Network failure affects only GPU installation;
CPU inference and conversations continue normally.

Selecting an uninstalled CUDA or Vulkan backend does not change the active or
pending backend. It opens a confirmation dialog showing backend, pack version,
download size, baseline, and the restart requirement.

After confirmation:

1. download to `.downloads/<backend>-<archive-sha256>.zip.part`;
2. resume with HTTP Range when the server and partial file permit it;
3. verify the archive size and SHA-256 from the pinned catalog;
4. safely extract to a unique staging directory;
5. reject traversal, absolute paths, links, duplicates, undeclared files, size
   excess, mixed backends, or missing dependencies;
6. validate `runtime-pack.json`, every declared file digest, and the live bridge
   probe;
7. replace the stable backend directory through the same journaled transaction;
8. store that backend as the pending backend and report `restart-required`.

The downloader accepts a partial only when its archive digest matches the selected
catalog entry and a resumed response has the expected `Content-Range` start. A
changed source, catalog, or digest cannot reuse an older partial.

Only one runtime installation may run at a time. Cancellation is cooperative
during download. Once final verification or rename begins, the operation completes
or rolls back instead of stopping midway. Closing the app preserves `.part`; the
next explicit installation request resumes it. Downloads never resume
automatically at startup.

An installed pack that fails inventory validation is shown as `repair-required`.
Repair uses the same confirmation pipeline. If that pack is loaded by the current
process, the validated staging directory and journal are retained as a deferred
replacement and applied at the start of the next process before worker creation.
An unloaded pack may be replaced immediately. An install or repair never mutates
the active backend selection before the new pack is fully valid.

## 8. Backend State and Switching

Pack lifecycle and user selection are separate state machines. The pack lifecycle
has these states:

- `ready`: installed, compatible, and selectable in this process;
- `not-installed`: no valid stable directory exists;
- `installing`: download or installation is active;
- `replacement-pending`: a validated pack is waiting for startup replacement;
- `repair-required`: installed files or live probe are invalid;
- `unavailable`: the pack is valid but its required device or driver is absent.

Selection state contains `activeBackend`, `desiredBackend`, and an optional
persisted `pendingActivation`. It is owned by Rust-side app-local settings rather
than React memory or WebView local storage. Installing a new pack writes
`pendingActivation` only after validation. On the next start, the app attempts it
once: success promotes it to active and clears pending; failure records the error,
marks that activation attempt consumed, runs CPU, and does not retry on every
start. The user can explicitly retry or select another backend.

Integrity, identity, and ABI failures map to `repair-required`. A structurally
valid pack whose driver or required device cannot be initialized maps to
`unavailable`; it is never offered as a repair unless file validation also fails.

CPU is the only automatic fallback. The resolver never silently falls from CUDA to
Vulkan or vice versa. If the saved backend is invalid or unavailable, CPU is used
for the current start and the saved preference is retained for diagnostics until
the user selects and applies another backend.

The settings panel presents CPU, CUDA, and Vulkan in one backend list. It does not
show a separate duplicate installer list. Each backend row owns its select,
install, repair, progress, cancellation, and restart action.

Already-ready backends use the existing orderly switch flow: cancel active
generation, persist its terminal state, unload the model and worker runtime, load
the selected pack, reload the model, and save the preference only after success.
If Windows refuses safe DLL replacement/unload, the app records the pending
backend and requests a restart instead.

Newly installed or repaired packs always require restart in this milestone. The
completion dialog offers `Restart now` and `Later`. Restart now first cancels an
active generation, waits for terminal persistence, then relaunches. The next
process consumes the pending backend only after inventory validates it. Later
keeps the current backend active and leaves a visible restart-required status.

## 9. Ownership Boundaries

- `runtime_baseline`: parse and validate build/bundled baseline metadata.
- `runtime_source`: load default/override source and construct approved GitHub
  Release URLs.
- `runtime_manifest`: parse and validate the remote catalog and pack manifest.
- `runtime_archive`: bounded extraction and archive/per-file integrity checks.
- `runtime_installer`: one-operation state machine, resume, cancellation, staging,
  deferred replacement, and journal creation.
- `runtime_transaction`: crash recovery, replacement, quarantine, and rollback.
- `runtime_bootstrap`: synchronize and recover the bundled CPU before worker
  creation.
- `runtime_packs`: inventory only the three stable IDs and perform live probes.
- `runtime_host`: optional worker ownership for `Ready` and `RecoveryRequired`
  startup modes.
- frontend runtime service/hook: map command events to backend lifecycle state.
- settings component: confirmation, progress, repair, and restart interactions.

The installer and bootstrap share archive validation and journaled replacement code.
Neither module owns the LLM worker. The worker consumes only a resolver result that
has already passed inventory validation.

## 10. Failure Rules

- Invalid local source override: report the override error in GPU installation UI;
  do not silently use the default and do not affect CPU.
- Manifest network/hash/schema failure: no install starts and current runtime is
  unchanged.
- Download cancellation/failure: stable pack is unchanged and resumable partial
  data may remain.
- Archive or probe failure: staging is removed and stable pack is unchanged.
- Replacement failure: quarantine the failed stable candidate, restore the
  validated backup, and surface repair/install failure.
- CPU bundle recovery failure with valid old CPU: continue using old CPU and
  report a nonfatal diagnostic.
- CPU recovery failure without valid CPU: do not spawn the worker; show a recovery
  error while preserving conversations and settings.
- Restart fails: keep the pending backend and tell the user to restart manually.

Errors shown to users identify the backend and corrective action. Full paths and
low-level loader diagnostics stay in the diagnostics view rather than the primary
settings message.

## 11. Validation Strategy

Validation is intentionally staged to avoid repeated full-suite loops.

### Focused checks during implementation

- baseline/source/manifest parser unit tests after those contracts change;
- archive and journaled replacement tests after installer changes;
- CPU bootstrap tests after startup integration;
- frontend reducer/component tests after lifecycle UI changes;
- one relevant Rust or TypeScript test command per completed unit.

Large archive hashing and native bridge probing are exercised only where their
logic changes. Playwright and complete workspace checks are not repeated after
every edit.

The focused tests include interrupted recovery after each rename phase, DLL-lock
deferred replacement, final-probe rollback, internal/external identity mismatch,
baseline commit or ABI-minor mismatch, fresh offline startup without an installed
CPU, failed pending activation followed by CPU fallback, `repair-required` versus
`unavailable`, stale partial rejection after source change, and exclusion of
`cpu-dev` and arbitrary IDs from inventory and fallback.

### Review checkpoint

After the design is written, perform one focused architecture review covering
baseline consistency, trust boundaries, journaled replacement, rollback, startup
ordering, and state transitions. Address only concrete correctness or missing-test
findings. Perform one final code review after implementation; do not create a
review/re-review loop for stylistic preferences.

### Final verification

Run once after all review fixes:

1. full Rust test suite and formatting/checks;
2. full frontend unit tests, typecheck, and production build;
3. PowerShell runtime release tests;
4. CPU bundle packaging smoke test;
5. one Playwright pass covering install confirmation, progress/cancel,
   restart-required, and repair states;
6. inspect the final Git diff and verify no generated test artifacts are committed.

CUDA/Vulkan hardware inference remains an environment-specific release smoke test.
The automated suite validates their manifests, archive contents, state flow, and
failure isolation without requiring those devices on every developer machine.

## 12. Completion Criteria

- A fresh Windows install can run the bundled CPU backend without network access.
- Production fallback and inventory use only `cpu`, `cuda`, and `vulkan`.
- CUDA/Vulkan selection installs only after explicit confirmation and preserves
  the current backend on every failure path.
- Every installed pack matches the pinned llama.cpp baseline and bridge ABI.
- Repository relocation requires only a valid source configuration change.
- Install and repair are resumable during download and rollback-safe during
  replacement.
- Newly installed/repaired GPU packs activate only after a controlled restart.
- Focused tests pass during development and the complete final verification passes
  once before completion.
