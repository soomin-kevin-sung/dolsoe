# Native Runtime Validation

All packs use the official llama.cpp `b10068` release at commit
`571d0d540df04f25298d0e159e520d9fc62ed121`. A pack directory contains the locally built
`local_llm_runtime.dll` bridge plus the checksum-pinned official release DLL set for CPU, CUDA 12.4, or
Vulkan. Official CPU packs contain instruction-set variants such as `ggml-cpu-x64.dll`; the loader selects
the compatible variant. Never combine DLLs from different llama.cpp releases. Changing CPU/CUDA/Vulkan
packs requires model unload and process/runtime restart; in-process backend-core replacement is unsupported.

## CPU
cmake -S native/llm-runtime -B .cmake-build/llm-cpu -A x64 -DLLW_BACKEND_PACK=CPU
cmake --build .cmake-build/llm-cpu --config Release
cmake --install .cmake-build/llm-cpu --config Release --prefix .runtime-packs/cpu-release

For desktop development, build, test, and activate the managed `cpu-dev` pack under the Tauri app-local data directory with:

```powershell
& scripts/prepare-dev-cpu-pack.ps1
```

Use `-Configuration Release`, `-PackId`, or `-DestinationRoot` only when a different development layout is required. The script verifies the complete CPU pack before replacing the active directory and restores the previous directory if activation fails.

## Conversation persistence smoke

Run the Tauri development app with the managed CPU pack and a verified tiny GGUF fixture:

```powershell
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
npm --prefix apps/desktop run tauri -- dev
```

Validate the native workflow in this order:

1. Select `$env:LLW_TEST_GGUF` and wait for the runtime to become ready.
2. Submit a prompt, wait for a terminal state, and create a second conversation with `Ctrl+N`.
3. Rename the second conversation, close the app, and start it again.
4. Confirm both conversation titles and the first conversation's user/assistant messages are restored.
5. Confirm stored messages remain visible before a model is selected after restart.

The SQLite database is stored at `app_local_data_dir/local-llm-wiki.db`. Startup migrations are repeatable, and assistant messages left in `streaming` state are recovered as `interrupted`.

## Installed runtime selection smoke

Validated on 2026-07-19 with Windows 11 x64 and an AMD Ryzen 5 5600X:

```powershell
& scripts/prepare-dev-cpu-pack.ps1
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
npm --prefix apps/desktop run tauri -- dev
```

Observed results:

1. Settings discovered `cpu-dev` under the managed app-local runtime root and displayed `AMD Ryzen 5 5600X 6-Core Processor`.
2. CPU was selected; CUDA and Vulkan were disabled with an installed-pack explanation because no ready GPU packs were present.
3. `tiny-random-f16.gguf` loaded through the selected CPU pack and reached `CPU · 준비됨`.
4. Restart rediscovered the CPU fallback and returned to the expected no-model state without a stale model path.
5. Diagnostics and automated tests covered pack ID/version/commit/ABI mapping, invalid preference fallback, and the `unload -> persist -> reload` transition order.

An actual generation-time switch to a different backend was hardware-gated because this machine had only one ready pack. The conversation layer still serializes cancellation through terminal message persistence before calling the tested runtime transition helper.

## Official backend pack assembly

The release builder compiles the bridge against the exact release commit, downloads the matching official
llama.cpp ZIP assets, verifies their SHA-256 digests, and places only DLLs plus the bridge test executable in
the final archive. CUDA combines the official CUDA 12.4 and matching `cudart` assets. No CUDA Toolkit, Vulkan
SDK, GPU, or self-hosted Actions runner is required to assemble a pack.

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
