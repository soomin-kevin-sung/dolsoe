# Native Runtime Validation

All packs use the official llama.cpp `b10068` release at commit
`571d0d540df04f25298d0e159e520d9fc62ed121`. A pack directory contains the locally built
`local_llm_runtime.dll` bridge plus the checksum-pinned official release DLL set for CPU, CUDA 12.4, or
Vulkan. The bridge is compiled against seven checksum-pinned public headers, but llama.cpp itself is not
cloned, built, or statically linked. At runtime the bridge loads `ggml-base.dll`, `ggml.dll`, and `llama.dll`
from the selected pack with absolute paths and resolves its required exports. Official CPU packs contain
instruction-set variants such as `ggml-cpu-x64.dll`; the loader selects the compatible variant. Never combine
DLLs from different llama.cpp releases. Switching between already installed CPU/CUDA/Vulkan packs unloads
the current model and runtime before loading the selected pack. A process restart is required only when an
installation replaces the backend DLL currently owned by the worker; first-time CPU recovery and installation
of an inactive backend are available to the running app immediately.

## CPU

Build the small bridge, run its tests beside the official DLLs, and assemble a complete CPU archive:

```powershell
& scripts/build-runtime-release.ps1 -Version 0.1.0-dev -Backend CPU -Configuration Release
Expand-Archive .runtime-release/dolsoe-runtime-0.1.0-dev-windows-x86_64-cpu.zip .runtime-packs/cpu-release
```

For desktop development, build, test, and activate the managed stable `cpu` pack under the Tauri app-local data directory with:

```powershell
& scripts/prepare-dev-cpu-pack.ps1
```

Use `-Configuration Release`, `-Force`, or `-DestinationRoot` only when a different development layout is required. The script reuses the release pack builder so `runtime-pack.json` and all payload hashes match the production contract. A valid current pack is reused; replacement restores the previous directory if activation fails.

## Conversation persistence smoke

Run the Tauri development app with the managed CPU pack and a verified tiny GGUF fixture:

```powershell
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
npm --prefix apps/desktop run desktop:dev
```

Validate the native workflow in this order:

1. Select `$env:LLW_TEST_GGUF` and wait for the runtime to become ready.
2. Submit a prompt, wait for a terminal state, and create a second conversation with `Ctrl+N`.
3. Rename the second conversation, close the app, and start it again.
4. Confirm both conversation titles and the first conversation's user/assistant messages are restored.
5. Confirm stored messages remain visible before a model is selected after restart.

## ReAct structured output

Runtime ABI 1.3 adds an optional bounded GBNF grammar to each generation request.
The desktop app sets this internal-only field for ReAct decisions and leaves it
unset for ordinary Chat generation. The grammar permits exactly one JSON object:
either a final response or a `calculator` tool call with an expression string.
It is applied by llama.cpp's grammar sampler before probability truncation, so
invalid keys, tool names, surrounding prose, and malformed JSON cannot be
sampled. The application still parses and validates the completed object because
generation can end early at the token budget or be cancelled.

Rebuild the development CPU pack after changing the ABI or grammar bridge:

```powershell
& scripts/prepare-dev-cpu-pack.ps1 -Force
```

Installed and development copies store the SQLite database at
`%LOCALAPPDATA%/Dolsoe/data/dolsoe.db`. On the first launch after this layout
change, the legacy Tauri app-local directory is moved into `Dolsoe/data`
atomically when both locations are on the same volume. A cross-volume install
copies into a staging directory and promotes the complete tree before removing
the legacy source. The main window is created only after migration and uses
`<data-root>/EBWebView`, preserving local storage without locking the legacy
directory. Startup database migrations remain repeatable, and assistant messages
left in `streaming` state are recovered as `interrupted`.

## Installed runtime selection smoke

Validated on 2026-07-19 with Windows 11 x64 and an AMD Ryzen 5 5600X:

```powershell
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
npm --prefix apps/desktop run desktop:dev
```

Observed results:

1. Settings discovered `cpu` under the managed app-local runtime root and displayed `AMD Ryzen 5 5600X 6-Core Processor`.
2. CPU was selected; CUDA and Vulkan were disabled with an installed-pack explanation because no ready GPU packs were present.
3. `tiny-random-f16.gguf` loaded through the selected CPU pack and reached `CPU · 준비됨`.
4. Restart rediscovered the CPU fallback and returned to the expected no-model state without a stale model path.
5. Diagnostics and automated tests covered pack ID/version/commit/ABI mapping, invalid preference fallback, and the `unload -> persist -> reload` transition order.

An actual generation-time switch to a different backend was hardware-gated because this machine had only one ready pack. The conversation layer still serializes cancellation through terminal message persistence before calling the tested runtime transition helper.

## Official backend pack assembly

The release builder downloads and verifies the exact public headers, compiles only the bridge, downloads the
matching official llama.cpp ZIP assets, verifies their SHA-256 digests, and places only DLLs plus the bridge
test executable in the final archive. CUDA combines the official CUDA 12.4 and matching `cudart` assets. No
llama.cpp source build, CUDA Toolkit, Vulkan SDK, GPU, or self-hosted Actions runner is required to assemble a
pack.

## Pack contents
$packs = @(
  @{ Path = '.runtime-packs/cpu-release'; Backend = $null },
  @{ Path = '.runtime-packs/cuda-release'; Backend = 'ggml-cuda.dll' },
  @{ Path = '.runtime-packs/vulkan-release'; Backend = 'ggml-vulkan.dll' }
)
foreach ($entry in $packs) {
  if (-not (Test-Path $entry.Path)) { continue }
  $required = @('local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll')
  if ($entry.Backend) { $required += $entry.Backend }
  $missing = $required | Select-Object -Unique | Where-Object { -not (Test-Path (Join-Path $entry.Path $_)) }
  if ($missing) { throw "$($entry.Path) is missing: $($missing -join ', ')" }
  if (-not (Get-ChildItem $entry.Path -Filter 'ggml-cpu*.dll')) { throw "$($entry.Path) has no CPU backend" }
  Get-ChildItem -File $entry.Path | Sort-Object Name | Select-Object Name,Length
}

## Hardware-gated runtime checks
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
$env:LLW_TEST_BACKEND = 'CUDA' # use VULKAN and its installed pack on a Vulkan-capable host
& .runtime-packs/cuda-release/llw_runtime_backend_test.exe $env:LLW_TEST_GGUF
if ($LASTEXITCODE -ne 0) { throw "hardware runtime test failed: $LASTEXITCODE" }

Pack assembly does not claim runtime GPU validation. Runtime CUDA/Vulkan tests use the explicit hardware-gated
command above. Metal is reserved for a future ABI-compatible macOS plan and is not configured or tested here.

## Signed runtime release assets

Build one Windows x64 asset at a time. The script configures, tests, stages, validates, and packages the selected backend:

```powershell
& scripts/build-runtime-release.ps1 -Version 2026.07.1 -Backend CPU
& scripts/build-runtime-release.ps1 -Version 2026.07.1 -Backend VULKAN
& scripts/build-runtime-release.ps1 -Version 2026.07.1 -Backend CUDA
```

All three assets run on pinned GitHub-hosted Windows runners. CUDA and Vulkan packs consume only official
llama.cpp release ZIPs; GPU drivers remain a runtime requirement on the user's machine.

Publishing requires a PKCS#8 Ed25519 private key encoded as base64. Store it only in the `LLW_RUNTIME_SIGNING_KEY` GitHub Actions secret. Never commit the private key or print the secret. To generate a signed manifest locally:

```powershell
$env:LLW_RUNTIME_SIGNING_KEY = '<base64 PKCS#8 DER private key>'
& scripts/generate-runtime-manifest.ps1 -Version 2026.07.1 -AssetDirectory .runtime-release
```

The command emits `runtime-manifest.json`, its detached base64 signature, and the raw public key encoded as base64. Production app builds must embed that public key through `LLW_RUNTIME_MANIFEST_PUBLIC_KEY` and set the manifest/signature URLs through `LLW_RUNTIME_MANIFEST_URL` and `LLW_RUNTIME_MANIFEST_SIGNATURE_URL`. Unsigned manifests are never accepted. Run the packaging fixture without compiling llama.cpp using:

```powershell
& scripts/tests/runtime-release.Tests.ps1
```

### Validation result (2026-07-19)

The Windows 11 x64 CPU Release path completed with Visual Studio, passed the native CTest suite, assembled the
official `b10068` CPU DLL variants, and loaded the checksum-pinned Tiny Random GGUF through the packaged bridge.
The manifest fixture and Rust archive installer accept the official CPU variant naming. No local CUDA or Vulkan
hardware runtime result is claimed; those checks remain hardware-gated.
