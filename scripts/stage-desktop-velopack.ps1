param(
  [string]$ApplicationExecutable = (Join-Path $PSScriptRoot '../target/release/dolsoe-desktop.exe'),
  [string]$RuntimeResourceDirectory = (Join-Path $PSScriptRoot '../apps/desktop/src-tauri/resources/runtime-packs'),
  [string]$OutputDirectory = (Join-Path $PSScriptRoot '../.desktop-package')
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$executablePath = [IO.Path]::GetFullPath($ApplicationExecutable)
if (-not (Test-Path -LiteralPath $executablePath -PathType Leaf)) {
  throw "Desktop executable does not exist: $executablePath"
}
if ([IO.Path]::GetExtension($executablePath) -ne '.exe') {
  throw "Desktop executable must be a Windows .exe: $executablePath"
}

$runtimeRoot = [IO.Path]::GetFullPath($RuntimeResourceDirectory)
$runtimeFiles = @('cpu.zip', 'cpu-index.json')
foreach ($name in $runtimeFiles) {
  $path = Join-Path $runtimeRoot $name
  if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
    throw "Bundled runtime resource does not exist: $path"
  }
}

$outputPath = [IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath $outputPath) {
  $existing = @(Get-ChildItem -LiteralPath $outputPath -Force)
  if ($existing.Count -gt 0) {
    throw "Velopack staging directory must be empty: $outputPath"
  }
} else {
  New-Item -ItemType Directory -Force -Path $outputPath | Out-Null
}

Copy-Item -LiteralPath $executablePath -Destination (Join-Path $outputPath ([IO.Path]::GetFileName($executablePath)))
$stagedRuntimeRoot = Join-Path $outputPath 'runtime-packs'
New-Item -ItemType Directory -Force -Path $stagedRuntimeRoot | Out-Null
foreach ($name in $runtimeFiles) {
  Copy-Item -LiteralPath (Join-Path $runtimeRoot $name) -Destination (Join-Path $stagedRuntimeRoot $name)
}

Write-Output $outputPath
