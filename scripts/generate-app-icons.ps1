param(
  [string]$SourceSvg = 'design/dolsoe-icon.svg',
  [string]$IconsDirectory = 'apps/desktop/src-tauri/icons',
  [string]$AppIcon = 'apps/desktop/src/assets/dolsoe-icon.svg'
)

# Regenerates the desktop icon assets from design/dolsoe-icon.svg.
# This intentionally leaves android/ and ios/ untouched.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$sourcePath = [IO.Path]::GetFullPath((Join-Path $repositoryRoot $SourceSvg))
$iconsRoot = [IO.Path]::GetFullPath((Join-Path $repositoryRoot $IconsDirectory))
$appIconPath = [IO.Path]::GetFullPath((Join-Path $repositoryRoot $AppIcon))
if (-not (Test-Path $sourcePath)) { throw "Source icon not found: $sourcePath" }
if (-not (Test-Path $iconsRoot)) { throw "Icons directory not found: $iconsRoot" }
if (-not (Test-Path (Split-Path -Parent $appIconPath))) { throw "App assets directory not found: $appIconPath" }

$stagingRoot = Join-Path ([IO.Path]::GetTempPath()) ("dolsoe-icons-" + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $stagingRoot | Out-Null
try {
  cargo-tauri icon $sourcePath -o $stagingRoot
  if ($LASTEXITCODE -ne 0) { throw "cargo-tauri icon failed with exit code $LASTEXITCODE." }

  $assets = @(
    'icon.icns', 'icon.ico', 'icon.png', '32x32.png', '64x64.png', '128x128.png', '128x128@2x.png',
    'Square30x30Logo.png', 'Square44x44Logo.png', 'Square71x71Logo.png', 'Square89x89Logo.png',
    'Square107x107Logo.png', 'Square142x142Logo.png', 'Square150x150Logo.png',
    'Square284x284Logo.png', 'Square310x310Logo.png', 'StoreLogo.png'
  )
  foreach ($asset in $assets) {
    $generated = Join-Path $stagingRoot $asset
    if (-not (Test-Path $generated)) { throw "Expected generated asset missing: $asset" }
    Copy-Item -LiteralPath $generated -Destination (Join-Path $iconsRoot $asset) -Force
    Write-Host "Updated $asset"
  }
  Copy-Item -LiteralPath $sourcePath -Destination $appIconPath -Force
  Write-Host "Updated $AppIcon"
} finally {
  Remove-Item -Recurse -Force $stagingRoot
}
