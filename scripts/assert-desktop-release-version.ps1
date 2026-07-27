param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$RuntimeTag
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$semanticVersionBody = '(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?'
$semanticVersionPattern = "^$semanticVersionBody$"
if ($Version -notmatch $semanticVersionPattern) {
  throw "Desktop version must be semantic version text without a leading v: $Version"
}
if ($RuntimeTag -notmatch "^runtime-v$semanticVersionBody$") {
  throw "Runtime tag must use runtime-v<semantic-version>: $RuntimeTag"
}

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$tauriConfigPath = Join-Path $repositoryRoot 'apps/desktop/src-tauri/tauri.conf.json'
$frontendPackagePath = Join-Path $repositoryRoot 'apps/desktop/package.json'
$cargoManifestPath = Join-Path $repositoryRoot 'apps/desktop/src-tauri/Cargo.toml'

$tauriVersion = (Get-Content -Raw -Encoding UTF8 $tauriConfigPath | ConvertFrom-Json).version
$frontendVersion = (Get-Content -Raw -Encoding UTF8 $frontendPackagePath | ConvertFrom-Json).version
$cargoSource = Get-Content -Raw -Encoding UTF8 $cargoManifestPath
$packageBlock = [regex]::Match($cargoSource, '(?ms)^\[package\][\r\n]+(?<body>.*?)(?=^\[|\z)')
if (-not $packageBlock.Success) { throw 'Desktop Cargo.toml is missing [package].' }
$cargoVersionMatch = [regex]::Match($packageBlock.Groups['body'].Value, '(?m)^version\s*=\s*"(?<version>[^"]+)"')
if (-not $cargoVersionMatch.Success) { throw 'Desktop Cargo.toml is missing package.version.' }
$cargoVersion = $cargoVersionMatch.Groups['version'].Value

$versions = [ordered]@{
  'tauri.conf.json' = [string]$tauriVersion
  'package.json' = [string]$frontendVersion
  'Cargo.toml' = [string]$cargoVersion
}
foreach ($entry in $versions.GetEnumerator()) {
  if ($entry.Value -ne $Version) {
    throw "$($entry.Key) version $($entry.Value) does not match desktop release $Version."
  }
}

[ordered]@{
  version = $Version
  runtimeTag = $RuntimeTag
  files = $versions
} | ConvertTo-Json -Depth 4 -Compress
