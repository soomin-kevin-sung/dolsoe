param(
  [Parameter(Mandatory)][string]$Version,
  [Parameter(Mandatory)][string]$AssetDirectory,
  [string]$OutputDirectory = $AssetDirectory,
  [string]$Repository = 'soomin-sung-estsoft/local-llm-wiki',
  [string]$MinimumAppVersion = '0.1.0',
  [string]$MaximumAppVersion = '0.1.x'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

if (-not $env:LLW_RUNTIME_SIGNING_KEY) {
  throw 'LLW_RUNTIME_SIGNING_KEY must contain a base64-encoded PKCS#8 Ed25519 private key.'
}
if ($Repository -notmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw 'Repository must be owner/name.' }

$assetRoot = [IO.Path]::GetFullPath($AssetDirectory)
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
New-Item -ItemType Directory -Force -Path $outputRoot | Out-Null
$escapedVersion = [Regex]::Escape($Version)
$assetPattern = "^local-llm-wiki-runtime-$escapedVersion-windows-x86_64-(cpu|cuda|vulkan)\.zip$"
$assets = Get-ChildItem -LiteralPath $assetRoot -File | Where-Object { $_.Name -match $assetPattern } | Sort-Object Name
if (-not $assets) { throw "No runtime ZIP assets found for version $Version." }

Add-Type -AssemblyName System.IO.Compression
Add-Type -AssemblyName System.IO.Compression.FileSystem
$packs = foreach ($asset in $assets) {
  $null = $asset.Name -match $assetPattern
  $backend = $Matches[1]
  $archive = [IO.Compression.ZipFile]::OpenRead($asset.FullName)
  try {
    $seen = @{}
    $files = foreach ($entry in ($archive.Entries | Where-Object { $_.Name } | Sort-Object FullName)) {
      $path = $entry.FullName.Replace('\', '/')
      if ($path.StartsWith('/') -or $path.Contains('../') -or $path.Contains('/..') -or $seen.ContainsKey($path)) {
        throw "Unsafe or duplicate ZIP entry in $($asset.Name): $path"
      }
      $seen[$path] = $true
      $stream = $entry.Open()
      $sha = [Security.Cryptography.SHA256]::Create()
      try { $hash = ([BitConverter]::ToString($sha.ComputeHash($stream)) -replace '-', '').ToLowerInvariant() } finally { $sha.Dispose(); $stream.Dispose() }
      [ordered]@{ path = $path; size = [long]$entry.Length; sha256 = $hash }
    }
  } finally {
    $archive.Dispose()
  }

  $names = @($files | ForEach-Object { $_.path })
  $required = @('local_llm_runtime.dll', 'llama.dll', 'ggml.dll', 'ggml-base.dll', 'llw_runtime_backend_test.exe')
  if ($backend -eq 'cuda') { $required += 'ggml-cuda.dll' }
  if ($backend -eq 'vulkan') { $required += 'ggml-vulkan.dll' }
  $missing = $required | Where-Object { $_ -notin $names }
  if ($missing) { throw "$($asset.Name) is missing required files: $($missing -join ', ')" }
  if (-not ($names | Where-Object { $_ -match '^ggml-cpu(?:-.+)?\.dll$' })) {
    throw "$($asset.Name) is missing a CPU backend DLL."
  }
  $forbidden = @('ggml-cuda.dll', 'ggml-vulkan.dll') | Where-Object { $_ -ne "ggml-$backend.dll" }
  if ($backend -eq 'cpu') { $forbidden = @('ggml-cuda.dll', 'ggml-vulkan.dll') }
  $mixed = $forbidden | Where-Object { $_ -in $names }
  if ($mixed) { throw "$($asset.Name) contains mixed backend DLLs: $($mixed -join ', ')" }
  if ($backend -eq 'cuda') {
    $missingCuda = @('^cublas64_.+\.dll$', '^cublasLt64_.+\.dll$', '^cudart64_.+\.dll$') | Where-Object { $pattern = $_; -not ($names | Where-Object { $_ -match $pattern }) }
    if ($missingCuda) { throw "$($asset.Name) is missing CUDA redistributable DLLs." }
  }

  [ordered]@{
    id = "$backend-$Version"
    backend = $backend
    platform = 'windows'
    arch = 'x86_64'
    assetUrl = "https://github.com/$Repository/releases/download/runtime-v$Version/$($asset.Name)"
    size = [long]$asset.Length
    sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $asset.FullName).Hash.ToLowerInvariant()
    files = @($files)
  }
}

$manifest = [ordered]@{
  schemaVersion = 1
  releaseVersion = $Version
  minimumAppVersion = $MinimumAppVersion
  maximumAppVersion = $MaximumAppVersion
  abiMajor = 1
  abiMinor = 1
  llamaCppCommit = '571d0d540df04f25298d0e159e520d9fc62ed121'
  packs = @($packs)
}

$manifestPath = Join-Path $outputRoot 'runtime-manifest.json'
$manifestBytes = [Text.UTF8Encoding]::new($false).GetBytes(($manifest | ConvertTo-Json -Depth 8))
[IO.File]::WriteAllBytes($manifestPath, $manifestBytes)

$openssl = if ($env:LLW_OPENSSL) { $env:LLW_OPENSSL } else {
  $command = Get-Command openssl -ErrorAction SilentlyContinue
  if ($command) { $command.Source } elseif (Test-Path 'C:\Program Files\Git\usr\bin\openssl.exe') { 'C:\Program Files\Git\usr\bin\openssl.exe' } else { $null }
}
if (-not $openssl) { throw 'OpenSSL 3 is required to sign the runtime manifest.' }

$keyPath = Join-Path $outputRoot ".runtime-signing-key-$PID.der"
$signatureBinary = Join-Path $outputRoot ".runtime-manifest-$PID.sig"
$publicDer = Join-Path $outputRoot ".runtime-public-key-$PID.der"
try {
  [IO.File]::WriteAllBytes($keyPath, [Convert]::FromBase64String($env:LLW_RUNTIME_SIGNING_KEY))
  & $openssl pkeyutl -sign -rawin -keyform DER -inkey $keyPath -in $manifestPath -out $signatureBinary
  if ($LASTEXITCODE -ne 0) { throw "OpenSSL manifest signing failed: $LASTEXITCODE" }
  [IO.File]::WriteAllText((Join-Path $outputRoot 'runtime-manifest.json.sig'), [Convert]::ToBase64String([IO.File]::ReadAllBytes($signatureBinary)), [Text.Encoding]::ASCII)

  & $openssl pkey -inform DER -in $keyPath -pubout -outform DER -out $publicDer
  if ($LASTEXITCODE -ne 0) { throw "OpenSSL public key export failed: $LASTEXITCODE" }
  $publicBytes = [IO.File]::ReadAllBytes($publicDer)
  if ($publicBytes.Length -lt 32) { throw 'Exported Ed25519 public key is invalid.' }
  $rawPublicKey = $publicBytes[($publicBytes.Length - 32)..($publicBytes.Length - 1)]
  [IO.File]::WriteAllText((Join-Path $outputRoot 'runtime-manifest.public-key'), [Convert]::ToBase64String($rawPublicKey), [Text.Encoding]::ASCII)
} finally {
  foreach ($path in @($keyPath, $signatureBinary, $publicDer)) {
    if (Test-Path -LiteralPath $path) { Remove-Item -Force -LiteralPath $path }
  }
}

Write-Output $manifestPath
