param(
  [Parameter(Mandatory)][string]$RuntimeDirectory,
  [Parameter(Mandatory)][string]$RuntimeTag,
  [Parameter(Mandatory)][string]$Repository,
  [string]$ResourceDirectory,
  [string]$RuntimeSourceDestination
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if (-not $ResourceDirectory) {
  $ResourceDirectory = Join-Path $repositoryRoot 'apps/desktop/src-tauri/resources/runtime-packs'
}
if (-not $RuntimeSourceDestination) {
  $RuntimeSourceDestination = Join-Path $repositoryRoot 'apps/desktop/src-tauri/resources/runtime-source.default.json'
}
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') {
  throw "Repository must use owner/name: $Repository"
}
if ($RuntimeTag -notmatch '^runtime-v(?<version>.+)$') {
  throw "Runtime tag must use runtime-v<version>: $RuntimeTag"
}
$runtimeVersion = $Matches['version']

$runtimeRoot = [IO.Path]::GetFullPath($RuntimeDirectory)
$manifestPath = Join-Path $runtimeRoot 'runtime-manifest.json'
$sourcePath = Join-Path $runtimeRoot 'runtime-source.json'
foreach ($path in @($manifestPath, $sourcePath)) {
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Runtime release is missing $([IO.Path]::GetFileName($path))."
  }
}

$source = Get-Content -Raw -Encoding UTF8 $sourcePath | ConvertFrom-Json
if ($source.schemaVersion -ne 1 -or $source.provider -ne 'github-release') {
  throw 'Runtime source has an unsupported schema or provider.'
}
if ($source.repository -ne $Repository) {
  throw "Runtime source repository $($source.repository) does not match $Repository."
}
if ($source.releaseTag -ne $RuntimeTag) {
  throw "Runtime source tag $($source.releaseTag) does not match $RuntimeTag."
}
if ($source.manifestAsset -ne 'runtime-manifest.json') {
  throw "Runtime source points to an unexpected manifest asset: $($source.manifestAsset)"
}
$manifestSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $manifestPath).Hash.ToLowerInvariant()
if ($source.manifestSha256 -ne $manifestSha256) {
  throw 'Runtime source does not pin the downloaded runtime manifest.'
}

$manifest = Get-Content -Raw -Encoding UTF8 $manifestPath | ConvertFrom-Json
if ($manifest.schemaVersion -ne 1 -or $manifest.releaseVersion -ne $runtimeVersion) {
  throw "Runtime manifest version does not match $RuntimeTag."
}
$cpuPacks = @($manifest.packs | Where-Object { $_.id -eq 'cpu' -and $_.backend -eq 'cpu' })
if ($cpuPacks.Count -ne 1) {
  throw 'Runtime manifest must contain exactly one CPU pack.'
}
$cpuAssetPath = Join-Path $runtimeRoot $cpuPacks[0].assetName
if (-not (Test-Path -LiteralPath $cpuAssetPath -PathType Leaf)) {
  throw "Runtime release is missing CPU asset $($cpuPacks[0].assetName)."
}

$bundler = Join-Path $repositoryRoot 'scripts/prepare-bundled-cpu-runtime.ps1'
& $bundler -RuntimeAsset $cpuAssetPath -Catalog $manifestPath -ResourceDirectory $ResourceDirectory | Out-Null

$sourceTarget = [IO.Path]::GetFullPath($RuntimeSourceDestination)
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $sourceTarget) | Out-Null
Copy-Item -Force -LiteralPath $sourcePath -Destination $sourceTarget

[ordered]@{
  runtimeTag = $RuntimeTag
  repository = $Repository
  cpuAsset = $cpuPacks[0].assetName
  manifestSha256 = $manifestSha256
} | ConvertTo-Json -Depth 3 -Compress
