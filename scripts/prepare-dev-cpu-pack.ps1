param(
  [string]$PackId = 'cpu-dev',
  [string]$DestinationRoot = (Join-Path $env:LOCALAPPDATA 'io.github.soomin-sung-estsoft.local-llm-wiki/runtime-packs'),
  [ValidateSet('Debug', 'Release')][string]$Configuration = 'Debug'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($PackId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$' -or $PackId.Contains('..')) {
  throw 'PackId must be 1-64 ASCII letters, digits, dots, underscores, or hyphens without traversal components.'
}
if ([string]::IsNullOrWhiteSpace($DestinationRoot)) {
  throw 'DestinationRoot must not be empty.'
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$sourceDirectory = Join-Path $repositoryRoot 'native/llm-runtime'
$buildDirectory = Join-Path $repositoryRoot '.cmake-build/llm-cpu-dev'
$destinationRootPath = [IO.Path]::GetFullPath($DestinationRoot)
New-Item -ItemType Directory -Force -Path $destinationRootPath | Out-Null
$destinationRootPath = (Resolve-Path -LiteralPath $destinationRootPath).Path.TrimEnd([IO.Path]::DirectorySeparatorChar)

$activePath = Join-Path $destinationRootPath $PackId
$stagingPath = Join-Path $destinationRootPath "$PackId.staging-$PID"
$backupPath = Join-Path $destinationRootPath "$PackId.backup-$PID"

function Assert-OwnedDirectChild([string]$Path, [string]$ExpectedName) {
  $fullPath = [IO.Path]::GetFullPath($Path)
  $parent = [IO.Path]::GetDirectoryName($fullPath).TrimEnd([IO.Path]::DirectorySeparatorChar)
  $name = [IO.Path]::GetFileName($fullPath)
  if (-not $parent.Equals($destinationRootPath, [StringComparison]::OrdinalIgnoreCase) -or $name -ne $ExpectedName) {
    throw "Refusing filesystem operation outside the managed runtime root: $fullPath"
  }
}

Assert-OwnedDirectChild $activePath $PackId
Assert-OwnedDirectChild $stagingPath "$PackId.staging-$PID"
Assert-OwnedDirectChild $backupPath "$PackId.backup-$PID"

foreach ($ownedPath in @($stagingPath, $backupPath)) {
  if (Test-Path -LiteralPath $ownedPath) {
    Remove-Item -Recurse -Force -LiteralPath $ownedPath
  }
}

& cmake -S $sourceDirectory -B $buildDirectory -A x64 -DLLW_BACKEND_PACK=CPU
if ($LASTEXITCODE -ne 0) { throw "CMake configure failed: $LASTEXITCODE" }
& cmake --build $buildDirectory --config $Configuration
if ($LASTEXITCODE -ne 0) { throw "CPU runtime build failed: $LASTEXITCODE" }
& ctest --test-dir $buildDirectory -C $Configuration --output-on-failure
if ($LASTEXITCODE -ne 0) { throw "CPU runtime tests failed: $LASTEXITCODE" }
& cmake --install $buildDirectory --config $Configuration --prefix $stagingPath
if ($LASTEXITCODE -ne 0) { throw "CPU runtime install failed: $LASTEXITCODE" }

$requiredFiles = @(
  'local_llm_runtime.dll',
  'llama.dll',
  'ggml.dll',
  'ggml-base.dll',
  'ggml-cpu.dll',
  'llw_runtime_backend_test.exe'
)
$missing = $requiredFiles | Where-Object { -not (Test-Path -LiteralPath (Join-Path $stagingPath $_) -PathType Leaf) }
if ($missing) {
  throw "CPU runtime staging pack is missing: $($missing -join ', ')"
}
$unexpected = @('ggml-cuda.dll', 'ggml-vulkan.dll') | Where-Object { Test-Path -LiteralPath (Join-Path $stagingPath $_) }
if ($unexpected) {
  throw "CPU runtime staging pack contains another backend: $($unexpected -join ', ')"
}

$movedActive = $false
try {
  if (Test-Path -LiteralPath $activePath) {
    Move-Item -LiteralPath $activePath -Destination $backupPath
    $movedActive = $true
  }
  Move-Item -LiteralPath $stagingPath -Destination $activePath
} catch {
  if ($movedActive -and -not (Test-Path -LiteralPath $activePath) -and (Test-Path -LiteralPath $backupPath)) {
    Move-Item -LiteralPath $backupPath -Destination $activePath
  }
  throw
}

if (Test-Path -LiteralPath $backupPath) {
  Remove-Item -Recurse -Force -LiteralPath $backupPath
}

Write-Output (Resolve-Path -LiteralPath $activePath).Path
