param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$RuntimeTag,
  [Parameter(Mandatory)][string]$CommitSha,
  [string]$PackId = 'Dolsoe',
  [string]$Channel = 'win-x64',
  [string]$OutputDirectory = (Join-Path $PSScriptRoot '../.desktop-release')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if ($Version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$') {
  throw "Invalid desktop release version: $Version"
}
if ($RuntimeTag -notmatch '^runtime-v.+$') {
  throw "Invalid runtime release tag: $RuntimeTag"
}
if ($CommitSha -notmatch '^[0-9a-fA-F]{40}$') {
  throw 'CommitSha must be a full 40-character Git commit SHA.'
}
if ($PackId -notmatch '^[A-Za-z0-9_.-]+$') {
  throw "Invalid Velopack pack ID: $PackId"
}
if ($Channel -notmatch '^[A-Za-z0-9_.-]+$') {
  throw "Invalid Velopack channel: $Channel"
}

$outputPath = [IO.Path]::GetFullPath($OutputDirectory)
if (-not (Test-Path -LiteralPath $outputPath -PathType Container)) {
  throw "Velopack output directory does not exist: $outputPath"
}

$setupName = "$PackId-$Channel-Setup.exe"
$fullPackageName = "$PackId-$Version-$Channel-full.nupkg"
$deltaPackageName = "$PackId-$Version-$Channel-delta.nupkg"
$feedName = "releases.$Channel.json"
$assetsName = "assets.$Channel.json"

foreach ($name in @($setupName, $fullPackageName, $feedName, $assetsName)) {
  $path = Join-Path $outputPath $name
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Velopack output is missing required asset: $name"
  }
}
if (Test-Path -LiteralPath (Join-Path $outputPath "$PackId-$Channel-Portable.zip")) {
  throw 'Portable output must stay disabled for desktop releases.'
}

$velopackAssets = Get-Content -Raw -Encoding UTF8 (Join-Path $outputPath $assetsName) | ConvertFrom-Json
$assetNames = @($velopackAssets | ForEach-Object { $_.RelativeFileName })
foreach ($name in @($setupName, $fullPackageName)) {
  if ($name -notin $assetNames) {
    throw "Velopack deployment metadata does not include required asset: $name"
  }
}

$publishedNames = @($setupName, $fullPackageName)
if (Test-Path -LiteralPath (Join-Path $outputPath $deltaPackageName) -PathType Leaf) {
  $publishedNames += $deltaPackageName
}
$publishedNames += $feedName

$manifestFiles = foreach ($name in $publishedNames) {
  $path = Join-Path $outputPath $name
  $kind = switch -Wildcard ($name) {
    '*-Setup.exe' { 'velopack-setup'; break }
    '*-full.nupkg' { 'velopack-full'; break }
    '*-delta.nupkg' { 'velopack-delta'; break }
    'releases.*.json' { 'velopack-feed'; break }
    default { 'velopack-asset' }
  }
  [ordered]@{
    name = $name
    kind = $kind
    size = (Get-Item -LiteralPath $path).Length
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash.ToLowerInvariant()
  }
}

$manifest = [ordered]@{
  schemaVersion = 1
  version = $Version
  runtimeTag = $RuntimeTag
  commitSha = $CommitSha.ToLowerInvariant()
  platform = 'windows'
  arch = 'x86_64'
  packaging = 'velopack'
  packId = $PackId
  channel = $Channel
  codeSigning = 'unsigned'
  files = @($manifestFiles)
}
$utf8 = [Text.UTF8Encoding]::new($false)
[IO.File]::WriteAllBytes(
  (Join-Path $outputPath 'desktop-release.json'),
  $utf8.GetBytes(($manifest | ConvertTo-Json -Depth 6))
)

$checksumLines = @($manifestFiles | Sort-Object name | ForEach-Object { "$($_.sha256)  $($_.name)" })
[IO.File]::WriteAllText(
  (Join-Path $outputPath 'SHA256SUMS.txt'),
  (($checksumLines -join "`n") + "`n"),
  $utf8
)

Write-Output $outputPath
