$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$builder = Join-Path $repositoryRoot 'scripts/build-runtime-release.ps1'
$generator = Join-Path $repositoryRoot 'scripts/generate-runtime-manifest.ps1'
$bundler = Join-Path $repositoryRoot 'scripts/prepare-bundled-cpu-runtime.ps1'
$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) "llw-runtime-release-tests-$PID"
$fixture = Join-Path $temporaryRoot 'cpu-pack'
$output = Join-Path $temporaryRoot 'output'
$cudaFixture = Join-Path $temporaryRoot 'cuda-pack'
$cudaOutput = Join-Path $temporaryRoot 'cuda-output'
$version = '2026.07.1'
$workflowPath = Join-Path $repositoryRoot '.github/workflows/runtime-release.yml'
$desktopWorkflowPath = Join-Path $repositoryRoot '.github/workflows/desktop-release.yml'
$ciWorkflowPath = Join-Path $repositoryRoot '.github/workflows/ci.yml'
$desktopBuildScriptPath = Join-Path $repositoryRoot 'apps/desktop/src-tauri/build.rs'
$desktopRuntimePreparerPath = Join-Path $repositoryRoot 'scripts/prepare-desktop-runtime.ps1'
$desktopPackagePath = Join-Path $repositoryRoot 'apps/desktop/package.json'
$devPreparerPath = Join-Path $repositoryRoot 'scripts/prepare-dev-cpu-pack.ps1'
$devLauncherPath = Join-Path $repositoryRoot 'scripts/start-desktop-dev.ps1'
$headerPreparerPath = Join-Path $repositoryRoot 'scripts/prepare-llama-headers.ps1'
$runtimeCmakePath = Join-Path $repositoryRoot 'native/llm-runtime/CMakeLists.txt'
$llamaApiPath = Join-Path $repositoryRoot 'native/llm-runtime/src/llama_api.cpp'
$runtimeValidationPath = Join-Path $repositoryRoot 'docs/native-runtime-validation.md'
$builderSource = Get-Content -Raw -Encoding UTF8 $builder
$baselinePath = Join-Path $repositoryRoot 'native/llm-runtime/llama-baseline.json'

function Assert-True([bool]$Condition, [string]$Message) {
  if (-not $Condition) { throw $Message }
}

function Assert-Throws([scriptblock]$Action, [string]$ExpectedMessage) {
  try {
    & $Action
  } catch {
    if ($_.Exception.Message -notlike "*$ExpectedMessage*") {
      throw "Expected error containing '$ExpectedMessage', got: $($_.Exception.Message)"
    }
    return
  }
  throw "Expected action to throw: $ExpectedMessage"
}

try {
  Assert-True (Test-Path -LiteralPath $baselinePath -PathType Leaf) 'llama.cpp baseline file is missing.'
  $baseline = Get-Content -Raw -Encoding UTF8 $baselinePath | ConvertFrom-Json
  Assert-True ($baseline.releaseTag -eq 'b10068') 'Unexpected llama.cpp baseline tag.'
  Assert-True ($baseline.commit -eq '571d0d540df04f25298d0e159e520d9fc62ed121') 'Unexpected llama.cpp baseline commit.'
  Assert-True ($baseline.abiMajor -eq 1 -and $baseline.abiMinor -eq 2) 'Unexpected bridge ABI baseline.'
  Assert-True (@($baseline.headers).Count -eq 7) 'Pinned llama.cpp header SDK must contain seven public headers.'
  foreach ($header in @($baseline.headers)) {
    Assert-True ([string]$header.sha256 -match '^[0-9a-f]{64}$') "Pinned header hash is malformed: $($header.path)"
  }

  New-Item -ItemType Directory -Force -Path $fixture, $output, $cudaFixture, $cudaOutput | Out-Null
  foreach ($name in @('local_llm_runtime.dll', 'llama.dll', 'ggml.dll', 'ggml-base.dll', 'ggml-cpu.dll', 'llw_runtime_backend_test.exe')) {
    [IO.File]::WriteAllText((Join-Path $fixture $name), "fixture-$name")
  }
  New-Item -ItemType Directory -Force -Path (Join-Path $fixture 'include') | Out-Null
  [IO.File]::WriteAllText((Join-Path $fixture 'include/development-only.h'), 'not-a-runtime-asset')

  & $builder -Version $version -Backend CPU -OutputRoot $output -SourcePack $fixture
  $assetName = "dolsoe-runtime-$version-windows-x86_64-cpu.zip"
  $assetPath = Join-Path $output $assetName
  Assert-True (Test-Path -LiteralPath $assetPath -PathType Leaf) 'Builder did not emit the deterministic CPU asset name.'
  Add-Type -AssemblyName System.IO.Compression.FileSystem
  $archive = [IO.Compression.ZipFile]::OpenRead($assetPath)
  try {
    $packEntry = $archive.GetEntry('runtime-pack.json')
    Assert-True ($null -ne $packEntry) 'Runtime archive omitted runtime-pack.json.'
    $reader = [IO.StreamReader]::new($packEntry.Open(), [Text.Encoding]::UTF8)
    try { $pack = $reader.ReadToEnd() | ConvertFrom-Json } finally { $reader.Dispose() }
  } finally { $archive.Dispose() }
  Assert-True ($pack.id -eq 'cpu') 'Runtime pack ID must be the stable backend ID.'
  Assert-True ($pack.backend -eq 'cpu') 'Runtime pack backend mismatch.'
  Assert-True ($pack.llamaCppCommit -eq $baseline.commit) 'Runtime pack baseline mismatch.'
  Assert-True ('runtime-pack.json' -notin $pack.files.path) 'Runtime pack manifest must not hash itself.'

  [IO.File]::WriteAllText((Join-Path $fixture 'ggml-cuda.dll'), 'mixed-backend')
  Assert-Throws { & $builder -Version $version -Backend CPU -OutputRoot $output -SourcePack $fixture } 'unexpected backend DLLs'
  Remove-Item -LiteralPath (Join-Path $fixture 'ggml-cuda.dll')

  foreach ($name in @('local_llm_runtime.dll', 'llama.dll', 'ggml.dll', 'ggml-base.dll', 'ggml-cpu.dll', 'ggml-cuda.dll', 'llw_runtime_backend_test.exe')) {
    [IO.File]::WriteAllText((Join-Path $cudaFixture $name), "fixture-$name")
  }
  Assert-Throws { & $builder -Version $version -Backend CUDA -OutputRoot $cudaOutput -SourcePack $cudaFixture } 'CUDA redistributable DLLs'
  foreach ($name in @('cublas64_13.dll', 'cublasLt64_13.dll', 'cudart64_13.dll')) {
    [IO.File]::WriteAllText((Join-Path $cudaFixture $name), "fixture-$name")
  }
  & $builder -Version $version -Backend CUDA -OutputRoot $cudaOutput -SourcePack $cudaFixture
  Assert-True (Test-Path -LiteralPath (Join-Path $cudaOutput "dolsoe-runtime-$version-windows-x86_64-cuda.zip")) 'CUDA asset was not created after redistributables were supplied.'

  $opensslCandidates = @(
    (Get-Command openssl -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Source -ErrorAction SilentlyContinue),
    'C:\Program Files\Git\usr\bin\openssl.exe'
  ) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) }
  $openssl = $opensslCandidates | Select-Object -First 1
  Assert-True ([bool]$openssl) 'OpenSSL 3 is required for the signing fixture.'

  $privateKey = Join-Path $temporaryRoot 'runtime-signing-key.der'
  & $openssl genpkey -algorithm ED25519 -outform DER -out $privateKey
  if ($LASTEXITCODE -ne 0) { throw 'Failed to generate fixture Ed25519 key.' }
  $env:LLW_RUNTIME_SIGNING_KEY = [Convert]::ToBase64String([IO.File]::ReadAllBytes($privateKey))
  $env:LLW_OPENSSL = $openssl

  & $generator -Version $version -AssetDirectory $output -OutputDirectory $output
  $manifestPath = Join-Path $output 'runtime-manifest.json'
  $sourcePath = Join-Path $output 'runtime-source.json'
  Assert-True (Test-Path -LiteralPath $manifestPath -PathType Leaf) 'Manifest was not generated.'
  Assert-True (Test-Path -LiteralPath $sourcePath -PathType Leaf) 'Runtime source was not generated.'

  $manifest = Get-Content -Raw -Encoding UTF8 $manifestPath | ConvertFrom-Json
  Assert-True ($manifest.schemaVersion -eq 1) 'Unexpected manifest schema version.'
  Assert-True ($manifest.releaseVersion -eq $version) 'Unexpected release version.'
  Assert-True ($manifest.packs.Count -eq 1) 'Expected exactly one fixture pack.'
  Assert-True ($manifest.packs[0].id -eq 'cpu') 'Catalog pack ID must be stable.'
  Assert-True ($manifest.packs[0].assetName -eq $assetName) 'Catalog asset name does not match the deterministic asset name.'
  Assert-True ($manifest.packs[0].sha256 -eq (Get-FileHash -Algorithm SHA256 $assetPath).Hash.ToLowerInvariant()) 'Archive hash mismatch.'
  Assert-True ('files' -notin $manifest.packs[0].PSObject.Properties.Name) 'Catalog must not duplicate payload file hashes.'
  $source = Get-Content -Raw -Encoding UTF8 $sourcePath | ConvertFrom-Json
  Assert-True ($source.manifestSha256 -eq (Get-FileHash -Algorithm SHA256 $manifestPath).Hash.ToLowerInvariant()) 'Runtime source did not pin the exact catalog hash.'

  $bundleResources = Join-Path $temporaryRoot 'bundle-resources'
  & $bundler -RuntimeAsset $assetPath -Catalog $manifestPath -ResourceDirectory $bundleResources
  Assert-True (Test-Path -LiteralPath (Join-Path $bundleResources 'cpu.zip') -PathType Leaf) 'CPU bundle archive was not prepared.'
  Assert-True (Test-Path -LiteralPath (Join-Path $bundleResources 'cpu-index.json') -PathType Leaf) 'CPU bundle index was not prepared.'

  Assert-True (Test-Path -LiteralPath $workflowPath -PathType Leaf) 'Runtime release workflow is missing.'
  $workflow = Get-Content -Raw -Encoding UTF8 $workflowPath
  $ciWorkflow = Get-Content -Raw -Encoding UTF8 $ciWorkflowPath
  Assert-True ($workflow -notmatch 'Visual Studio 17 2022') 'Runtime release must not force an unavailable Visual Studio generator.'
  Assert-True ($ciWorkflow -notmatch 'Visual Studio 17 2022') 'CI must not force an unavailable Visual Studio generator.'
  Assert-True ($workflow -match 'windows-2025') 'Runtime assets must use pinned hosted Windows runners.'
  Assert-True ($workflow -notmatch 'self-hosted') 'Official llama.cpp DLL packs must not require a private runner.'
  Assert-True ($workflow -notmatch 'CUDA_PATH|nvcc|VULKAN_SDK') 'Runtime packaging must not install vendor toolkits.'
  Assert-True ($ciWorkflow -notmatch 'self-hosted|CUDA_PATH|nvcc|VULKAN_SDK') 'CI backend packaging must not require vendor toolkits.'
  Assert-True ('llama-b10068-bin-win-cuda-12.4-x64.zip' -in $baseline.assets.cuda.name) 'CUDA must use the pinned official llama.cpp release asset.'
  Assert-True ('cudart-llama-bin-win-cuda-12.4-x64.zip' -in $baseline.assets.cuda.name) 'CUDA must include the matching official CUDA runtime asset.'
  Assert-True ($builderSource -notmatch 'CUDA_PATH|nvcc|VULKAN_SDK') 'Runtime builder must not use locally installed vendor toolkits.'
  Assert-True ($workflow -match 'actions/upload-artifact@[0-9a-f]{40}') 'Build artifacts must use a commit-pinned upload action.'
  Assert-True ($workflow -match 'actions/download-artifact@[0-9a-f]{40}') 'Publish must collect commit-pinned build artifacts.'
  Assert-True ($workflow -notmatch 'LLW_RUNTIME_SIGNING_KEY|runtime-manifest\.json\.sig|runtime-manifest\.public-key') 'Runtime publishing must not require signing keys or detached signature assets.'
  Assert-True ($workflow -match '\.runtime-release/runtime-manifest\.json') 'Publish must include the pinned runtime catalog.'
  Assert-True ($workflow -match 'contents: write') 'Only the publish job may write release contents.'
  Assert-True ($workflow -match 'Refusing to overwrite existing release') 'Publishing must refuse to overwrite a release.'

  Assert-True (Test-Path -LiteralPath $desktopWorkflowPath -PathType Leaf) 'Desktop release workflow is missing.'
  $desktopWorkflow = Get-Content -Raw -Encoding UTF8 $desktopWorkflowPath
  $desktopBuildScript = Get-Content -Raw -Encoding UTF8 $desktopBuildScriptPath
  $desktopRuntimePreparer = Get-Content -Raw -Encoding UTF8 $desktopRuntimePreparerPath
  Assert-True ($desktopWorkflow -match 'runtime_tag') 'Desktop release must select an immutable runtime release tag.'
  Assert-True ($desktopWorkflow -match 'gh release download') 'Desktop release must download runtime release assets.'
  Assert-True ($desktopRuntimePreparer -match 'runtime-source\.default\.json') 'Desktop release must inject the pinned runtime source.'
  Assert-True ($desktopRuntimePreparer -match 'Get-FileHash' -and $desktopRuntimePreparer -match 'manifestSha256') 'Desktop release must bind runtime-source.json to the downloaded catalog bytes.'
  Assert-True ($desktopRuntimePreparer -match 'prepare-bundled-cpu-runtime\.ps1') 'Desktop release must prepare the bundled CPU pack.'
  Assert-True ($desktopWorkflow -match 'tauri -- build --ci') 'Desktop release must build the Tauri application after preparing runtime resources.'
  Assert-True ($desktopWorkflow -match 'stage-desktop-velopack\.ps1') 'Desktop release must stage bundled runtime resources for Velopack.'
  Assert-True ($desktopWorkflow -match 'package-desktop-release\.ps1') 'Desktop release must describe Velopack assets and emit checksums.'
  Assert-True ($desktopWorkflow -match 'vpk upload github') 'Desktop release must publish Velopack assets to GitHub Releases.'
  Assert-True ($desktopWorkflow -notmatch 'tauri version') 'Desktop release must not call the read-only Tauri version command to mutate configuration.'
  Assert-True ($desktopBuildScript -match 'PROFILE') 'The Tauri build script must distinguish release packaging.'
  Assert-True ($desktopBuildScript -match 'cpu\.zip' -and $desktopBuildScript -match 'cpu-index\.json') 'Release builds must require bundled CPU resources.'
  Assert-True ($desktopBuildScript -match '0000000000000000000000000000000000000000000000000000000000000000') 'Release builds must reject the placeholder runtime source digest.'
  Assert-True ($desktopBuildScript -match 'len\(\) != 64' -and $desktopBuildScript -match 'is_ascii') 'Release builds must reject malformed non-placeholder source digests.'

  $devPreparerSource = Get-Content -Raw -Encoding UTF8 $devPreparerPath
  $devLauncherSource = Get-Content -Raw -Encoding UTF8 $devLauncherPath
  $headerPreparerSource = Get-Content -Raw -Encoding UTF8 $headerPreparerPath
  $runtimeCmakeSource = Get-Content -Raw -Encoding UTF8 $runtimeCmakePath
  $llamaApiSource = Get-Content -Raw -Encoding UTF8 $llamaApiPath
  $desktopPackage = Get-Content -Raw -Encoding UTF8 $desktopPackagePath | ConvertFrom-Json
  $runtimeValidation = Get-Content -Raw -Encoding UTF8 $runtimeValidationPath
  $parseTokens = $null
  $parseErrors = $null
  [Management.Automation.Language.Parser]::ParseFile($devPreparerPath, [ref]$parseTokens, [ref]$parseErrors) | Out-Null
  Assert-True ($parseErrors.Count -eq 0) 'Development CPU pack preparer has PowerShell syntax errors.'
  $parseTokens = $null
  $parseErrors = $null
  [Management.Automation.Language.Parser]::ParseFile($devLauncherPath, [ref]$parseTokens, [ref]$parseErrors) | Out-Null
  Assert-True ($parseErrors.Count -eq 0) 'Desktop development launcher has PowerShell syntax errors.'
  Assert-True ($devPreparerSource -match '\$packId\s*=\s*''cpu''' -and $devPreparerSource -notmatch '\[string\]\$PackId') 'Development must install only the stable CPU pack ID.'
  Assert-True ($devPreparerSource -match 'build-runtime-release\.ps1' -and $devPreparerSource -match 'runtime-pack\.json') 'Development CPU preparation must reuse the release manifest contract.'
  Assert-True ($devPreparerSource -notmatch 'FetchContent|llama\.cpp\.git') 'Development CPU preparation must not clone or compile llama.cpp.'
  Assert-True ($builderSource -match 'prepare-llama-headers\.ps1' -and $builderSource -match 'LLW_LLAMA_INCLUDE_DIR') 'Runtime builds must use the checksum-pinned header SDK.'
  Assert-True ($headerPreparerSource -match 'raw\.githubusercontent\.com' -and $headerPreparerSource -match 'Get-FileHash') 'Header SDK preparation must download exact sources and verify checksums.'
  Assert-True ($runtimeCmakeSource -notmatch 'FetchContent|add_subdirectory\([^\r\n]*llama|target_link_libraries\([^\r\n]*\bllama\b') 'The bridge must not build or link llama.cpp.'
  Assert-True ($runtimeCmakeSource -match 'src/llama_api\.cpp') 'The dynamic llama.cpp loader must be compiled into the bridge.'
  Assert-True ($llamaApiSource -match 'LoadLibraryExW' -and $llamaApiSource -match 'GetProcAddress') 'The bridge must resolve official llama.cpp release DLL exports at runtime.'
  Assert-True ($devLauncherSource -match 'prepare-dev-cpu-pack\.ps1' -and $devLauncherSource -match 'npm run tauri -- dev') 'Desktop development launcher must prepare CPU before Tauri.'
  Assert-True ($desktopPackage.scripts.'desktop:dev' -match 'start-desktop-dev\.ps1') 'Desktop package must expose the integrated development command.'
  Assert-True ($runtimeValidation -notmatch 'cpu-dev' -and $runtimeValidation -match 'desktop:dev') 'Runtime validation docs must use the stable integrated development flow.'

  Write-Output 'runtime release fixture tests passed'
} finally {
  Remove-Item Env:LLW_RUNTIME_SIGNING_KEY -ErrorAction SilentlyContinue
  Remove-Item Env:LLW_OPENSSL -ErrorAction SilentlyContinue
  if (Test-Path -LiteralPath $temporaryRoot) {
    Remove-Item -Recurse -Force -LiteralPath $temporaryRoot
  }
}
