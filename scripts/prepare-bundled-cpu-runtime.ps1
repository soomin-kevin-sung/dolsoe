param(
  [Parameter(Mandatory)][string]$RuntimeAsset,
  [Parameter(Mandatory)][string]$Catalog,
  [string]$ResourceDirectory = (Join-Path $PSScriptRoot '../apps/desktop/src-tauri/resources/runtime-packs')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$asset = Get-Item -LiteralPath ([IO.Path]::GetFullPath($RuntimeAsset))
$catalogValue = Get-Content -Raw -Encoding UTF8 ([IO.Path]::GetFullPath($Catalog)) | ConvertFrom-Json
$cpu = @($catalogValue.packs | Where-Object { $_.id -eq 'cpu' -and $_.backend -eq 'cpu' })
if ($cpu.Count -ne 1) { throw 'Runtime catalog must contain exactly one CPU pack.' }
if ($cpu[0].assetName -ne $asset.Name) { throw 'CPU catalog asset name does not match the selected archive.' }
$actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $asset.FullName).Hash.ToLowerInvariant()
if ($actual -ne $cpu[0].sha256) { throw 'CPU catalog archive SHA-256 mismatch.' }

$target = [IO.Path]::GetFullPath($ResourceDirectory)
New-Item -ItemType Directory -Force -Path $target | Out-Null
Copy-Item -Force -LiteralPath $asset.FullName -Destination (Join-Path $target 'cpu.zip')
$indexBytes = [Text.UTF8Encoding]::new($false).GetBytes(($cpu[0] | ConvertTo-Json -Depth 8))
[IO.File]::WriteAllBytes((Join-Path $target 'cpu-index.json'), $indexBytes)
Write-Output $target
