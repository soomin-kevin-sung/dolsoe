param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][ValidateSet('CPU', 'CUDA', 'VULKAN')][string]$Backend,
  [string]$OutputRoot = (Join-Path $PSScriptRoot '../.runtime-release'),
  [string]$SourcePack,
  [string]$BuildDirectory,
  [ValidateSet('Debug', 'Release')][string]$Configuration = 'Release'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$llamaReleaseTag = 'b10068'
$llamaReleaseBaseUrl = "https://github.com/ggml-org/llama.cpp/releases/download/$llamaReleaseTag"
$llamaReleaseAssets = @{
  CPU = @(
    @{ Name = 'llama-b10068-bin-win-cpu-x64.zip'; Sha256 = '01d5f30876acfb4a0be59396710f450213495c7181d8fbcce2fad045835ceb89' }
  )
  VULKAN = @(
    @{ Name = 'llama-b10068-bin-win-vulkan-x64.zip'; Sha256 = '4f3e6fd215fdf22d2fd6232a5501f9e791a93d9193db4faf59e391eff90f6169' }
  )
  CUDA = @(
    @{ Name = 'llama-b10068-bin-win-cuda-12.4-x64.zip'; Sha256 = 'a249fb8d3f072d2746e8bd93af3f901eadaff7dedc7ff27a415af488da2d8411' },
    @{ Name = 'cudart-llama-bin-win-cuda-12.4-x64.zip'; Sha256 = '8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6' }
  )
}

function Install-LlamaReleaseDlls([string]$Pack, [string]$BackendName, [string]$CacheRoot) {
  New-Item -ItemType Directory -Force -Path $CacheRoot | Out-Null
  Get-ChildItem -LiteralPath $Pack -Filter '*.dll' -File |
    Where-Object Name -ne 'local_llm_runtime.dll' |
    Remove-Item -Force

  foreach ($asset in $llamaReleaseAssets[$BackendName]) {
    $archivePath = Join-Path $CacheRoot $asset.Name
    if (-not (Test-Path -LiteralPath $archivePath -PathType Leaf)) {
      Invoke-WebRequest -Uri "$llamaReleaseBaseUrl/$($asset.Name)" -OutFile "$archivePath.part"
      Move-Item -Force -LiteralPath "$archivePath.part" -Destination $archivePath
    }
    $actual = (Get-FileHash -LiteralPath $archivePath -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($actual -ne $asset.Sha256) {
      Remove-Item -Force -LiteralPath $archivePath
      throw "Official llama.cpp asset checksum mismatch for $($asset.Name): $actual"
    }

    $extractPath = Join-Path $CacheRoot ".extract-$BackendName-$PID-$($asset.Name)"
    if (Test-Path -LiteralPath $extractPath) { Remove-Item -Recurse -Force -LiteralPath $extractPath }
    Expand-Archive -LiteralPath $archivePath -DestinationPath $extractPath
    try {
      $dlls = Get-ChildItem -LiteralPath $extractPath -Recurse -Filter '*.dll' -File
      if (-not $dlls) { throw "Official llama.cpp asset contains no DLLs: $($asset.Name)" }
      foreach ($dll in $dlls) {
        Copy-Item -Force -LiteralPath $dll.FullName -Destination (Join-Path $Pack $dll.Name)
      }
    } finally {
      Remove-Item -Recurse -Force -LiteralPath $extractPath
    }
  }
}

if ($Version -notmatch '^\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$') {
  throw 'Version must be a semantic version without a leading v.'
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$outputRootPath = [IO.Path]::GetFullPath($OutputRoot)
New-Item -ItemType Directory -Force -Path $outputRootPath | Out-Null
$backendLower = $Backend.ToLowerInvariant()
$packPath = if ($SourcePack) { [IO.Path]::GetFullPath($SourcePack) } else { Join-Path $outputRootPath ".staging-$backendLower-$PID" }

if (-not $SourcePack) {
  $buildDirectoryPath = if ($BuildDirectory) { [IO.Path]::GetFullPath($BuildDirectory) } else { Join-Path $repositoryRoot ".cmake-build/llm-$backendLower-release" }
  if (Test-Path -LiteralPath $packPath) { Remove-Item -Recurse -Force -LiteralPath $packPath }
  & cmake -S (Join-Path $repositoryRoot 'native/llm-runtime') -B $buildDirectoryPath -A x64 "-DLLW_BACKEND_PACK=$Backend"
  if ($LASTEXITCODE -ne 0) { throw "CMake configure failed for $Backend`: $LASTEXITCODE" }
  & cmake --build $buildDirectoryPath --config $Configuration
  if ($LASTEXITCODE -ne 0) { throw "$Backend runtime build failed: $LASTEXITCODE" }
  & ctest --test-dir $buildDirectoryPath -C $Configuration --output-on-failure
  if ($LASTEXITCODE -ne 0) { throw "$Backend runtime tests failed: $LASTEXITCODE" }
  & cmake --install $buildDirectoryPath --config $Configuration --prefix $packPath
  if ($LASTEXITCODE -ne 0) { throw "$Backend runtime install failed: $LASTEXITCODE" }
  Install-LlamaReleaseDlls -Pack $packPath -BackendName $Backend -CacheRoot (Join-Path $outputRootPath ".llama-release-$llamaReleaseTag")
}

if (-not (Test-Path -LiteralPath $packPath -PathType Container)) {
  throw "Runtime pack directory does not exist: $packPath"
}

$required = @('local_llm_runtime.dll', 'llama.dll', 'ggml.dll', 'ggml-base.dll', 'llw_runtime_backend_test.exe')
if ($Backend -eq 'CUDA') { $required += 'ggml-cuda.dll' }
if ($Backend -eq 'VULKAN') { $required += 'ggml-vulkan.dll' }
$missing = $required | Where-Object { -not (Test-Path -LiteralPath (Join-Path $packPath $_) -PathType Leaf) }
if ($missing) { throw "$Backend runtime pack is missing required files: $($missing -join ', ')" }
$cpuBackends = Get-ChildItem -LiteralPath $packPath -Filter 'ggml-cpu*.dll' -File
if (-not $cpuBackends) { throw "$Backend runtime pack is missing a CPU backend DLL." }

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
