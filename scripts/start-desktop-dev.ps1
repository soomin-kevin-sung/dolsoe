param(
  [ValidateSet('Debug', 'Release')][string]$Configuration = 'Debug',
  [switch]$ForceRuntime
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$prepare = Join-Path $PSScriptRoot 'prepare-dev-cpu-pack.ps1'
if ($ForceRuntime) {
  & $prepare -Configuration $Configuration -Force
} else {
  & $prepare -Configuration $Configuration
}

Push-Location (Join-Path $repositoryRoot 'apps/desktop')
try {
  & npm run tauri -- dev
  if ($LASTEXITCODE -ne 0) {
    throw "Tauri development app exited with code $LASTEXITCODE."
  }
} finally {
  Pop-Location
}
