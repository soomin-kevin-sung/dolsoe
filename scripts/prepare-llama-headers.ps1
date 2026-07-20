param(
  [string]$OutputDirectory
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$baselinePath = Join-Path $repositoryRoot 'native/llm-runtime/llama-baseline.json'
$baseline = Get-Content -Raw -Encoding UTF8 $baselinePath | ConvertFrom-Json
if ($baseline.schemaVersion -ne 1 -or -not $baseline.headers) {
  throw 'llama.cpp baseline is missing the pinned header SDK.'
}

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
  $OutputDirectory = Join-Path $repositoryRoot ".runtime-packs/llama-headers-$($baseline.releaseTag)"
}
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$outputRoot = (Resolve-Path -LiteralPath $outputRoot).Path

foreach ($header in @($baseline.headers)) {
  $name = [string]$header.path
  $sourcePath = [string]$header.sourcePath
  $expected = ([string]$header.sha256).ToLowerInvariant()
  if ([IO.Path]::GetFileName($name) -ne $name -or $sourcePath -notmatch '^[A-Za-z0-9._/-]+$') {
    throw "Invalid pinned header path: $name"
  }
  if ($expected -notmatch '^[0-9a-f]{64}$') {
    throw "Invalid pinned header SHA-256: $name"
  }

  $destination = Join-Path $outputRoot $name
  if (Test-Path -LiteralPath $destination -PathType Leaf) {
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $destination).Hash.ToLowerInvariant()
    if ($actual -eq $expected) { continue }
  }

  $temporary = "$destination.part-$PID"
  Remove-Item -Force -LiteralPath $temporary -ErrorAction SilentlyContinue
  $url = "https://raw.githubusercontent.com/ggml-org/llama.cpp/$($baseline.commit)/$sourcePath"
  try {
    Invoke-WebRequest -Uri $url -OutFile $temporary
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $temporary).Hash.ToLowerInvariant()
    if ($actual -ne $expected) {
      throw "Pinned llama.cpp header checksum mismatch for $name`: $actual"
    }
    Move-Item -Force -LiteralPath $temporary -Destination $destination
  } finally {
    Remove-Item -Force -LiteralPath $temporary -ErrorAction SilentlyContinue
  }
}

Write-Output $outputRoot
