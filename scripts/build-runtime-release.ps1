param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][ValidateSet('CPU', 'CUDA', 'VULKAN')][string]$Backend,
  [string]$OutputRoot = (Join-Path $PSScriptRoot '../.runtime-release'),
  [string]$SourcePack,
  [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($Version -notmatch '^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$') {
  throw 'Version must be a semantic version without a leading v.'
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$outputRootPath = [IO.Path]::GetFullPath($OutputRoot)
New-Item -ItemType Directory -Force -Path $outputRootPath | Out-Null
$backendLower = $Backend.ToLowerInvariant()
$packPath = if ($SourcePack) { [IO.Path]::GetFullPath($SourcePack) } else { Join-Path $outputRootPath ".staging-$backendLower-$PID" }

if (-not $SourcePack) {
  $buildDirectory = Join-Path $repositoryRoot ".cmake-build/llm-$backendLower-release"
  if (Test-Path -LiteralPath $packPath) { Remove-Item -Recurse -Force -LiteralPath $packPath }
  & cmake -S (Join-Path $repositoryRoot 'native/llm-runtime') -B $buildDirectory -A x64 "-DLLW_BACKEND_PACK=$Backend"
  if ($LASTEXITCODE -ne 0) { throw "CMake configure failed for $Backend`: $LASTEXITCODE" }
  & cmake --build $buildDirectory --config $Configuration
  if ($LASTEXITCODE -ne 0) { throw "$Backend runtime build failed: $LASTEXITCODE" }
  & ctest --test-dir $buildDirectory -C $Configuration --output-on-failure
  if ($LASTEXITCODE -ne 0) { throw "$Backend runtime tests failed: $LASTEXITCODE" }
  & cmake --install $buildDirectory --config $Configuration --prefix $packPath
  if ($LASTEXITCODE -ne 0) { throw "$Backend runtime install failed: $LASTEXITCODE" }
  if ($Backend -eq 'CUDA') {
    if (-not $env:CUDA_PATH) { throw 'CUDA_PATH is required to collect CUDA redistributable DLLs.' }
    $cudaBin = Join-Path $env:CUDA_PATH 'bin'
    foreach ($pattern in @('cublas64_*.dll', 'cublasLt64_*.dll', 'cudart64_*.dll')) {
      $redistributables = Get-ChildItem -LiteralPath $cudaBin -Filter $pattern -File
      if (-not $redistributables) { throw "CUDA redistributable DLLs are missing for pattern $pattern." }
      Copy-Item -LiteralPath $redistributables.FullName -Destination $packPath
    }
  }
}

if (-not (Test-Path -LiteralPath $packPath -PathType Container)) {
  throw "Runtime pack directory does not exist: $packPath"
}

$required = @('local_llm_runtime.dll', 'llama.dll', 'ggml.dll', 'ggml-base.dll', 'ggml-cpu.dll', 'llw_runtime_backend_test.exe')
if ($Backend -eq 'CUDA') { $required += 'ggml-cuda.dll' }
if ($Backend -eq 'VULKAN') { $required += 'ggml-vulkan.dll' }
$missing = $required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $packPath $_) -PathType Leaf) }
if ($missing) { throw "$Backend runtime pack is missing required files: $($missing -join ', ')" }

$forbidden = switch ($Backend) {
  'CPU' { @('ggml-cuda.dll', 'ggml-vulkan.dll') }
  'CUDA' { @('ggml-vulkan.dll') }
  'VULKAN' { @('ggml-cuda.dll') }
}
$unexpected = $forbidden | Where-Object { Test-Path -LiteralPath (Join-Path $packPath $_) -PathType Leaf }
if ($unexpected) { throw "$Backend runtime pack contains unexpected backend DLLs: $($unexpected -join ', ')" }
if ($Backend -eq 'CUDA') {
  $missingCuda = @('cublas64_*.dll', 'cublasLt64_*.dll', 'cudart64_*.dll') | Where-Object { -not (Get-ChildItem -LiteralPath $packPath -Filter $_ -File) }
  if ($missingCuda) { throw "CUDA redistributable DLLs are missing: $($missingCuda -join ', ')" }
}

$assetName = "local-llm-wiki-runtime-$Version-windows-x86_64-$backendLower.zip"
$assetPath = Join-Path $outputRootPath $assetName
$temporaryAsset = "$assetPath.part"
if (Test-Path -LiteralPath $temporaryAsset) { Remove-Item -Force -LiteralPath $temporaryAsset }

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [IO.Compression.ZipFile]::Open($temporaryAsset, [IO.Compression.ZipArchiveMode]::Create)
try {
  $files = Get-ChildItem -LiteralPath $packPath -File | Where-Object { $_.Name -ne 'THIRD_PARTY_NOTICES.txt' } | Sort-Object Name
  foreach ($file in $files) {
    $entry = $archive.CreateEntry($file.Name, [IO.Compression.CompressionLevel]::Optimal)
    $entry.LastWriteTime = [DateTimeOffset]::new(2020, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
    $input = $file.OpenRead()
    $output = $entry.Open()
    try { $input.CopyTo($output) } finally { $output.Dispose(); $input.Dispose() }
  }
  $notice = Get-Item -LiteralPath (Join-Path $repositoryRoot 'THIRD_PARTY_NOTICES.txt')
  $entry = $archive.CreateEntry('THIRD_PARTY_NOTICES.txt', [IO.Compression.CompressionLevel]::Optimal)
  $entry.LastWriteTime = [DateTimeOffset]::new(2020, 1, 1, 0, 0, 0, [TimeSpan]::Zero)
  $input = $notice.OpenRead()
  $output = $entry.Open()
  try { $input.CopyTo($output) } finally { $output.Dispose(); $input.Dispose() }
} finally {
  $archive.Dispose()
}

Move-Item -Force -LiteralPath $temporaryAsset -Destination $assetPath
if (-not $SourcePack -and (Test-Path -LiteralPath $packPath)) { Remove-Item -Recurse -Force -LiteralPath $packPath }
Write-Output $assetPath
