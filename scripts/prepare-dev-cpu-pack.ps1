param(
  [string]$DestinationRoot = (Join-Path $env:LOCALAPPDATA 'io.github.soomin-kevin-sung.local-llm-wiki/runtime-packs'),
  [ValidateSet('Debug', 'Release')][string]$Configuration = 'Debug',
  [switch]$Force
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ([string]::IsNullOrWhiteSpace($DestinationRoot)) {
  throw 'DestinationRoot must not be empty.'
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$package = Get-Content -Raw -Encoding UTF8 (Join-Path $repositoryRoot 'apps/desktop/package.json') | ConvertFrom-Json
$baseline = Get-Content -Raw -Encoding UTF8 (Join-Path $repositoryRoot 'native/llm-runtime/llama-baseline.json') | ConvertFrom-Json
$packId = 'cpu'
$packVersion = "$($package.version)-dev"
$buildDirectory = Join-Path $repositoryRoot '.cmake-build/llm-cpu-dev'
$outputRoot = Join-Path $repositoryRoot '.runtime-packs/dev-cpu-pack'
$destinationRootPath = [IO.Path]::GetFullPath($DestinationRoot)
New-Item -ItemType Directory -Force -Path $destinationRootPath | Out-Null
$destinationRootPath = (Resolve-Path -LiteralPath $destinationRootPath).Path.TrimEnd([IO.Path]::DirectorySeparatorChar)

$activePath = Join-Path $destinationRootPath $packId
$stagingName = "$packId.staging-$PID"
$backupName = "$packId.backup-$PID"
$stagingPath = Join-Path $destinationRootPath $stagingName
$backupPath = Join-Path $destinationRootPath $backupName

function Assert-OwnedDirectChild([string]$Path, [string]$ExpectedName) {
  $fullPath = [IO.Path]::GetFullPath($Path)
  $parent = [IO.Path]::GetDirectoryName($fullPath).TrimEnd([IO.Path]::DirectorySeparatorChar)
  $name = [IO.Path]::GetFileName($fullPath)
  if (-not $parent.Equals($destinationRootPath, [StringComparison]::OrdinalIgnoreCase) -or $name -ne $ExpectedName) {
    throw "Refusing filesystem operation outside the managed runtime root: $fullPath"
  }
}

function Test-DevCpuPack([string]$Path) {
  if (-not (Test-Path -LiteralPath $Path -PathType Container)) { return $false }
  try {
    $manifest = Get-Content -Raw -Encoding UTF8 (Join-Path $Path 'runtime-pack.json') | ConvertFrom-Json
    if ($manifest.schemaVersion -ne 1 -or $manifest.id -ne $packId -or $manifest.backend -ne $packId) { return $false }
    if ($manifest.packVersion -ne $packVersion -or $manifest.platform -ne $baseline.platform -or $manifest.arch -ne $baseline.arch) { return $false }
    if ($manifest.llamaCppRelease -ne $baseline.releaseTag -or $manifest.llamaCppCommit -ne $baseline.commit) { return $false }
    if ($manifest.abiMajor -ne $baseline.abiMajor -or $manifest.abiMinor -ne $baseline.abiMinor) { return $false }

    $declared = @($manifest.files)
    if ($declared.Count -eq 0) { return $false }
    $names = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach ($file in $declared) {
      $name = [string]$file.path
      if ([string]::IsNullOrWhiteSpace($name) -or [IO.Path]::GetFileName($name) -ne $name -or -not $names.Add($name)) { return $false }
      $candidate = Join-Path $Path $name
      if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) { return $false }
      $item = Get-Item -LiteralPath $candidate
      if ($item.Length -ne [long]$file.size) { return $false }
      $actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $candidate).Hash.ToLowerInvariant()
      if ($actualHash -ne [string]$file.sha256) { return $false }
    }

    foreach ($required in @('local_llm_runtime.dll', 'llama.dll', 'ggml.dll', 'ggml-base.dll', 'llw_runtime_backend_test.exe', 'THIRD_PARTY_NOTICES.txt')) {
      if (-not $names.Contains($required)) { return $false }
    }
    if (-not (Get-ChildItem -LiteralPath $Path -Filter 'ggml-cpu*.dll' -File)) { return $false }
    if (Test-Path -LiteralPath (Join-Path $Path 'ggml-cuda.dll') -PathType Leaf) { return $false }
    if (Test-Path -LiteralPath (Join-Path $Path 'ggml-vulkan.dll') -PathType Leaf) { return $false }
    return $true
  } catch {
    return $false
  }
}

Assert-OwnedDirectChild $activePath $packId
Assert-OwnedDirectChild $stagingPath $stagingName
Assert-OwnedDirectChild $backupPath $backupName

if (-not $Force -and (Test-DevCpuPack $activePath)) {
  Write-Output $activePath
  return
}

foreach ($ownedPath in @($stagingPath, $backupPath)) {
  if (Test-Path -LiteralPath $ownedPath) {
    Remove-Item -Recurse -Force -LiteralPath $ownedPath
  }
}

$builder = Join-Path $PSScriptRoot 'build-runtime-release.ps1'
$builderArguments = @{
  Version = $packVersion
  Backend = 'CPU'
  OutputRoot = $outputRoot
  BuildDirectory = $buildDirectory
  Configuration = $Configuration
}
& $builder @builderArguments
$assetPath = Join-Path $outputRoot "local-llm-wiki-runtime-$packVersion-windows-x86_64-cpu.zip"
if (-not (Test-Path -LiteralPath $assetPath -PathType Leaf)) {
  throw "CPU runtime builder did not produce the expected archive: $assetPath"
}

Expand-Archive -LiteralPath $assetPath -DestinationPath $stagingPath
if (-not (Test-DevCpuPack $stagingPath)) {
  Remove-Item -Recurse -Force -LiteralPath $stagingPath
  throw 'Generated CPU runtime pack failed development validation.'
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
