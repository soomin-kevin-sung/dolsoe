# Native Runtime Validation

All packs use llama.cpp `6bdd77f13cf11b264b4231d320afc404f48d576e`. A pack directory contains one
`local_llm_runtime.dll`, `llama.dll`, `ggml.dll`, `ggml-base.dll`, `ggml-cpu.dll`, and only its selected
GPU backend DLL/dependencies. Never combine DLLs from different build directories. Changing CPU/CUDA/Vulkan
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

## CUDA compile smoke
cmake -S native/llm-runtime -B .cmake-build/llm-cuda -A x64 -DLLW_BACKEND_PACK=CUDA
cmake --build .cmake-build/llm-cuda --config Release
cmake --install .cmake-build/llm-cuda --config Release --prefix .runtime-packs/cuda-release

## Vulkan compile smoke
$version = '1.4.350.0'
$url = 'https://sdk.lunarg.com/sdk/download/1.4.350.0/windows/vulkansdk-windows-X64-1.4.350.0.exe'
$sha256 = '855b27ba05d2d8119c5114c5d4ff870ca38f2c632b11e1bb9923b9b7e6ecfe7b'
$installer = Join-Path $env:RUNNER_TEMP 'vulkan-sdk.exe'
Invoke-WebRequest -Uri $url -OutFile $installer
if ((Get-FileHash -Algorithm SHA256 $installer).Hash.ToLowerInvariant() -ne $sha256) { throw 'Vulkan SDK checksum mismatch' }
$root = "C:\VulkanSDK\$version"
$process = Start-Process -Wait -PassThru -FilePath $installer -ArgumentList '--root', $root, '--accept-licenses', '--default-answer', '--confirm-command', 'install'
if ($process.ExitCode -ne 0) { throw "Vulkan SDK installer failed: $($process.ExitCode)" }
$env:VULKAN_SDK = $root
cmake -S native/llm-runtime -B .cmake-build/llm-vulkan -A x64 -DLLW_BACKEND_PACK=VULKAN
cmake --build .cmake-build/llm-vulkan --config Release
cmake --install .cmake-build/llm-vulkan --config Release --prefix .runtime-packs/vulkan-release

## Pack contents
$packs = @(
  @{ Path = '.runtime-packs/cpu-release'; Backend = 'ggml-cpu.dll' },
  @{ Path = '.runtime-packs/cuda-release'; Backend = 'ggml-cuda.dll' },
  @{ Path = '.runtime-packs/vulkan-release'; Backend = 'ggml-vulkan.dll' }
)
foreach ($entry in $packs) {
  if (-not (Test-Path $entry.Path)) { continue }
  $required = 'local_llm_runtime.dll','llama.dll','ggml.dll','ggml-base.dll','ggml-cpu.dll',$entry.Backend
  $missing = $required | Select-Object -Unique | Where-Object { -not (Test-Path (Join-Path $entry.Path $_)) }
  if ($missing) { throw "$($entry.Path) is missing: $($missing -join ', ')" }
  Get-ChildItem -File $entry.Path | Sort-Object Name | Select-Object Name,Length
}

## Hardware-gated runtime checks
$env:LLW_TEST_GGUF = & scripts/acquire-test-model.ps1
$env:LLW_TEST_BACKEND = 'CUDA' # use VULKAN and its installed pack on a Vulkan-capable host
& .runtime-packs/cuda-release/llw_runtime_backend_test.exe $env:LLW_TEST_GGUF
if ($LASTEXITCODE -ne 0) { throw "hardware runtime test failed: $LASTEXITCODE" }

The Vulkan SDK source, version, 324012984-byte size, and SHA-256 are pinned from
`https://vulkan.lunarg.com/sdk/files.json`; unattended arguments are from
`https://vulkan.lunarg.com/doc/view/1.4.350.0/windows/getting_started.html`.
The CUDA command requires the self-hosted runner labels `Windows`, `X64`, and `cuda`, plus `nvcc` and
`CUDA_PATH`. Compile smoke does not claim runtime GPU validation. Runtime CUDA/Vulkan tests use the explicit
hardware-gated command above. Metal is reserved for a future ABI-compatible macOS plan and is not configured,
compiled, or tested here.
