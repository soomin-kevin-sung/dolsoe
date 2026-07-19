param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$AssetDirectory,
  [string]$OutputDirectory = $AssetDirectory,
  [string]$MinimumAppVersion = '0.1.0',
  [string]$MaximumAppVersion = '0.1.x'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$baseline = Get-Content -Raw -Encoding UTF8 (Join-Path $repositoryRoot 'native/llm-runtime/llama-baseline.json') | ConvertFrom-Json
if ($baseline.schemaVersion -ne 1) { throw "Unsupported llama.cpp baseline schema: $($baseline.schemaVersion)" }

$assetRoot = [IO.Path]::GetFullPath($AssetDirectory)
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$escapedVersion = [Regex]::Escape($Version)
$assetPattern = "^local-llm-wiki-runtime-$escapedVersion-windows-x86_64-(cpu|cuda|vulkan)\.zip$"
$assets = Get-ChildItem -LiteralPath $assetRoot -File | Where-Object { $_.Name -match $assetPattern } | Sort-Object Name
if (-not $assets) { throw "No runtime ZIP assets found for version $Version." }

Add-Type -AssemblyName System.IO.Compression.FileSystem
$packs = foreach ($asset in $assets) {
  $null = $asset.Name -match $assetPattern
  $backend = $Matches[1]
  $archive = [IO.Compression.ZipFile]::OpenRead($asset.FullName)
  try {
    $manifestEntry = $archive.GetEntry('runtime-pack.json')
    if ($null -eq $manifestEntry) { throw "$($asset.Name) is missing runtime-pack.json." }
    $reader = [IO.StreamReader]::new($manifestEntry.Open(), [Text.Encoding]::UTF8)
    try { $pack = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
  } finally { $archive.Dispose() }

  if ($pack.id -ne $backend -or $pack.backend -ne $backend) { throw "$($asset.Name) has a mismatched stable backend identity." }
  if ($pack.packVersion -ne $Version) { throw "$($asset.Name) has a mismatched pack version." }
  if ($pack.platform -ne $baseline.platform -or $pack.arch -ne $baseline.arch) { throw "$($asset.Name) has a mismatched platform." }
  if ($pack.llamaCppRelease -ne $baseline.releaseTag -or $pack.llamaCppCommit -ne $baseline.commit) { throw "$($asset.Name) has a mismatched llama.cpp baseline." }
  if ($pack.abiMajor -ne $baseline.abiMajor -or $pack.abiMinor -ne $baseline.abiMinor) { throw "$($asset.Name) has a mismatched bridge ABI." }

  [ordered]@{
    id = $backend
    backend = $backend
    packVersion = $Version
    platform = $pack.platform
    arch = $pack.arch
    llamaCppRelease = $pack.llamaCppRelease
    llamaCppCommit = $pack.llamaCppCommit
    abiMajor = [int]$pack.abiMajor
    abiMinor = [int]$pack.abiMinor
    assetName = $asset.Name
    size = [long]$asset.Length
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $asset.FullName).Hash.ToLowerInvariant()
  }
}

$manifest = [ordered]@{
  schemaVersion = 1
  releaseVersion = $Version
  minimumAppVersion = $MinimumAppVersion
  maximumAppVersion = $MaximumAppVersion
  packs = @($packs)
}

$manifestPath = Join-Path $outputRoot 'runtime-manifest.json'
$manifestBytes = [Text.UTF8Encoding]::new($false).GetBytes(($manifest | ConvertTo-Json -Depth 8))
[IO.File]::WriteAllBytes($manifestPath, $manifestBytes)
Write-Output $manifestPath
