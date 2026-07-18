param([string]$Destination = '.test-models/tiny-random-f16.gguf')
$ErrorActionPreference = 'Stop'
$manifest = Get-Content -Raw 'native/llm-runtime/tests/fixtures/model.json' | ConvertFrom-Json
$destinationPath = [IO.Path]::GetFullPath((Join-Path (Get-Location) $Destination))
$directory = Split-Path -Parent $destinationPath
New-Item -ItemType Directory -Force $directory | Out-Null
if (Test-Path $destinationPath) {
  $existing = Get-Item $destinationPath
  $existingHash = (Get-FileHash -Algorithm SHA256 $destinationPath).Hash.ToLowerInvariant()
  if ($existing.Length -eq [int64]$manifest.size -and $existingHash -eq $manifest.sha256) {
    Write-Output $destinationPath
    exit 0
  }
  Remove-Item -LiteralPath $destinationPath
}
$temporary = "$destinationPath.download"
Invoke-WebRequest -Uri $manifest.url -OutFile $temporary
$file = Get-Item $temporary
if ($file.Length -ne [int64]$manifest.size) { Remove-Item -LiteralPath $temporary; throw "fixture size mismatch" }
$actual = (Get-FileHash -Algorithm SHA256 $temporary).Hash.ToLowerInvariant()
if ($actual -ne $manifest.sha256) { Remove-Item -LiteralPath $temporary; throw "fixture SHA-256 mismatch: $actual" }
Move-Item -Force -LiteralPath $temporary -Destination $destinationPath
Write-Output $destinationPath
