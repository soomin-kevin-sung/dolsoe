$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$temporaryBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
$temporaryRoot = Join-Path $temporaryBase "dolsoe-desktop-release-tests-$PID-$([guid]::NewGuid().ToString('N'))"
$workflowPath = Join-Path $repositoryRoot '.github/workflows/desktop-release.yml'
$ciWorkflowPath = Join-Path $repositoryRoot '.github/workflows/ci.yml'
$tauriConfigPath = Join-Path $repositoryRoot 'apps/desktop/src-tauri/tauri.conf.json'
$cargoManifestPath = Join-Path $repositoryRoot 'apps/desktop/src-tauri/Cargo.toml'
$mainSourcePath = Join-Path $repositoryRoot 'apps/desktop/src-tauri/src/main.rs'
$versionScript = Join-Path $repositoryRoot 'scripts/assert-desktop-release-version.ps1'
$runtimeScript = Join-Path $repositoryRoot 'scripts/prepare-desktop-runtime.ps1'
$stageScript = Join-Path $repositoryRoot 'scripts/stage-desktop-velopack.ps1'
$packageScript = Join-Path $repositoryRoot 'scripts/package-desktop-release.ps1'

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
  New-Item -ItemType Directory -Force -Path $temporaryRoot | Out-Null

  foreach ($script in @($versionScript, $runtimeScript, $stageScript, $packageScript)) {
    $tokens = $null
    $errors = $null
    [Management.Automation.Language.Parser]::ParseFile($script, [ref]$tokens, [ref]$errors) | Out-Null
    Assert-True ($errors.Count -eq 0) "$([IO.Path]::GetFileName($script)) has PowerShell syntax errors."
  }

  & $versionScript -Version '0.1.0' -RuntimeTag 'runtime-v0.1.0' | Out-Null
  Assert-Throws {
    & $versionScript -Version '0.1.1' -RuntimeTag 'runtime-v0.1.0'
  } 'does not match desktop release'
  Assert-Throws {
    & $versionScript -Version 'v0.1.0' -RuntimeTag 'runtime-v0.1.0'
  } 'without a leading v'

  $runtimeDirectory = Join-Path $temporaryRoot 'runtime'
  $runtimeResources = Join-Path $temporaryRoot 'runtime-resources'
  $runtimeSourceTarget = Join-Path $temporaryRoot 'runtime-source.default.json'
  New-Item -ItemType Directory -Force -Path $runtimeDirectory | Out-Null
  $cpuAssetName = 'dolsoe-runtime-0.1.0-windows-x86_64-cpu.zip'
  $cpuAssetPath = Join-Path $runtimeDirectory $cpuAssetName
  [IO.File]::WriteAllText($cpuAssetPath, 'desktop-runtime-fixture')
  $cpuSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $cpuAssetPath).Hash.ToLowerInvariant()
  $manifest = [ordered]@{
    schemaVersion = 1
    releaseVersion = '0.1.0'
    packs = @(
      [ordered]@{
        id = 'cpu'
        backend = 'cpu'
        assetName = $cpuAssetName
        sha256 = $cpuSha256
      }
    )
  }
  $utf8 = [Text.UTF8Encoding]::new($false)
  $manifestPath = Join-Path $runtimeDirectory 'runtime-manifest.json'
  [IO.File]::WriteAllBytes($manifestPath, $utf8.GetBytes(($manifest | ConvertTo-Json -Depth 6)))
  $manifestSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $manifestPath).Hash.ToLowerInvariant()
  $source = [ordered]@{
    schemaVersion = 1
    provider = 'github-release'
    repository = 'soomin-kevin-sung/dolsoe'
    releaseTag = 'runtime-v0.1.0'
    manifestAsset = 'runtime-manifest.json'
    manifestSha256 = $manifestSha256
  }
  [IO.File]::WriteAllBytes(
    (Join-Path $runtimeDirectory 'runtime-source.json'),
    $utf8.GetBytes(($source | ConvertTo-Json -Depth 4))
  )

  & $runtimeScript `
    -RuntimeDirectory $runtimeDirectory `
    -RuntimeTag 'runtime-v0.1.0' `
    -Repository 'soomin-kevin-sung/dolsoe' `
    -ResourceDirectory $runtimeResources `
    -RuntimeSourceDestination $runtimeSourceTarget | Out-Null
  Assert-True (Test-Path -LiteralPath (Join-Path $runtimeResources 'cpu.zip')) 'CPU runtime was not bundled.'
  Assert-True (Test-Path -LiteralPath (Join-Path $runtimeResources 'cpu-index.json')) 'CPU runtime index was not generated.'
  Assert-True (Test-Path -LiteralPath $runtimeSourceTarget) 'Pinned runtime source was not copied.'
  Assert-Throws {
    & $runtimeScript `
      -RuntimeDirectory $runtimeDirectory `
      -RuntimeTag 'runtime-v0.1.0' `
      -Repository 'someone-else/dolsoe' `
      -ResourceDirectory (Join-Path $temporaryRoot 'wrong-runtime') `
      -RuntimeSourceDestination (Join-Path $temporaryRoot 'wrong-source.json')
  } 'does not match'

  $applicationRoot = Join-Path $temporaryRoot 'application'
  $applicationExecutable = Join-Path $applicationRoot 'dolsoe-desktop.exe'
  $runtimeResourceRoot = Join-Path $temporaryRoot 'bundled-runtime'
  $stagingRoot = Join-Path $temporaryRoot 'staging'
  $releaseRoot = Join-Path $temporaryRoot 'release'
  New-Item -ItemType Directory -Force -Path $applicationRoot, $runtimeResourceRoot, $releaseRoot | Out-Null
  [IO.File]::WriteAllText($applicationExecutable, 'desktop-executable-fixture')
  [IO.File]::WriteAllText((Join-Path $runtimeResourceRoot 'cpu.zip'), 'runtime-archive-fixture')
  [IO.File]::WriteAllText((Join-Path $runtimeResourceRoot 'cpu-index.json'), '{}')

  & $stageScript `
    -ApplicationExecutable $applicationExecutable `
    -RuntimeResourceDirectory $runtimeResourceRoot `
    -OutputDirectory $stagingRoot | Out-Null
  Assert-True (Test-Path -LiteralPath (Join-Path $stagingRoot 'dolsoe-desktop.exe')) 'Desktop executable was not staged.'
  Assert-True (Test-Path -LiteralPath (Join-Path $stagingRoot 'runtime-packs/cpu.zip')) 'CPU runtime was not staged.'
  Assert-True (Test-Path -LiteralPath (Join-Path $stagingRoot 'runtime-packs/cpu-index.json')) 'CPU runtime index was not staged.'

  $setupName = 'Dolsoe-win-x64-Setup.exe'
  $fullPackageName = 'Dolsoe-0.1.0-win-x64-full.nupkg'
  $feedName = 'releases.win-x64.json'
  [IO.File]::WriteAllText((Join-Path $releaseRoot $setupName), 'setup-fixture')
  [IO.File]::WriteAllText((Join-Path $releaseRoot $fullPackageName), 'full-package-fixture')
  [IO.File]::WriteAllText((Join-Path $releaseRoot $feedName), '{"Assets":[]}')
  [IO.File]::WriteAllText(
    (Join-Path $releaseRoot 'assets.win-x64.json'),
    "[{`"RelativeFileName`":`"$setupName`",`"Type`":`"Installer`"},{`"RelativeFileName`":`"$fullPackageName`",`"Type`":`"Full`"}]"
  )

  & $packageScript `
    -Version '0.1.0' `
    -RuntimeTag 'runtime-v0.1.0' `
    -CommitSha ('a' * 40) `
    -OutputDirectory $releaseRoot | Out-Null
  $releaseManifest = Get-Content -Raw -Encoding UTF8 (Join-Path $releaseRoot 'desktop-release.json') | ConvertFrom-Json
  Assert-True ($releaseManifest.version -eq '0.1.0') 'Desktop release manifest has the wrong version.'
  Assert-True ($releaseManifest.runtimeTag -eq 'runtime-v0.1.0') 'Desktop release manifest has the wrong runtime tag.'
  Assert-True ($releaseManifest.packaging -eq 'velopack') 'Desktop release must identify Velopack packaging.'
  Assert-True ($releaseManifest.channel -eq 'win-x64') 'Desktop release has the wrong Velopack channel.'
  Assert-True ($releaseManifest.codeSigning -eq 'unsigned') 'Desktop release must disclose unsigned code signing.'
  Assert-True (@($releaseManifest.files).Count -eq 3) 'Desktop release manifest must contain setup, full package, and feed.'
  Assert-True ((Get-Content -Encoding UTF8 (Join-Path $releaseRoot 'SHA256SUMS.txt')).Count -eq 3) 'Checksums must cover published Velopack assets.'

  $workflow = Get-Content -Raw -Encoding UTF8 $workflowPath
  $ciWorkflow = Get-Content -Raw -Encoding UTF8 $ciWorkflowPath
  $tauriConfig = Get-Content -Raw -Encoding UTF8 $tauriConfigPath | ConvertFrom-Json
  $cargoManifest = Get-Content -Raw -Encoding UTF8 $cargoManifestPath
  $mainSource = Get-Content -Raw -Encoding UTF8 $mainSourcePath
  Assert-True ('active' -notin $tauriConfig.bundle.PSObject.Properties.Name) 'Ordinary Tauri builds must use the non-bundling Tauri default.'
  Assert-True ('targets' -notin $tauriConfig.bundle.PSObject.Properties.Name) 'Tauri bundle targets must not drive desktop releases.'
  Assert-True ('resources' -notin $tauriConfig.bundle.PSObject.Properties.Name) 'Velopack staging must own release resources.'
  Assert-True ($cargoManifest -match 'velopack\s*=\s*"=1\.2\.0"') 'The Velopack Rust SDK must be version-pinned.'
  Assert-True ($mainSource -match 'fn main\(\)\s*\{\s*velopack::VelopackApp::build\(\)\.run\(\);') 'Velopack startup hooks must run first.'
  Assert-True ($workflow -match "tags:\s*[\r\n]+\s*-\s*'desktop-v\*'") 'Desktop tags must trigger releases.'
  Assert-True ($workflow -match 'workflow_dispatch' -and $workflow -match 'runtime_tag') 'Manual releases must select a runtime tag.'
  Assert-True ($workflow -match 'assert-desktop-release-version\.ps1') 'Release metadata must validate app versions.'
  Assert-True ($workflow -match 'prepare-desktop-runtime\.ps1') 'Release builds must validate and bundle the runtime.'
  Assert-True ($workflow -match 'stage-desktop-velopack\.ps1') 'Release builds must stage the Velopack application.'
  Assert-True ($workflow -match 'package-desktop-release\.ps1') 'Release builds must finalize deterministic assets.'
  Assert-True ($workflow -match 'dotnet tool install --global vpk --version 1\.2\.0') 'The Velopack CLI must match the Rust SDK.'
  Assert-True ($workflow -match "'download', 'github'" -and $workflow -match 'vpk pack' -and $workflow -match 'vpk upload github') 'The workflow must use the Velopack deployment sequence.'
  Assert-True ($workflow -match 'VPK_TOKEN:\s*\$\{\{\s*github\.token\s*\}\}' -and $workflow -notmatch '--token') 'Velopack credentials must use the protected environment.'
  Assert-True ($workflow -match 'tauri -- build --ci') 'Tauri must build only the release application.'
  Assert-True ($workflow -notmatch 'bundles nsis|bundles msi|gh release create') 'Legacy Tauri installers must not be published.'
  Assert-True ($workflow -match '--channel win-x64' -and $workflow -match '--runtime win-x64') 'Velopack releases must isolate the Windows x64 channel.'
  Assert-True ($workflow -match '--framework webview2' -and $workflow -match '--noPortable') 'Velopack must bootstrap WebView2 and omit duplicate portable bundles.'
  Assert-True ($workflow -match 'npm run test:unit' -and $workflow -match 'cargo test --locked') 'Release builds must run frontend and backend tests.'
  Assert-True ($workflow -match 'actions/upload-artifact@[0-9a-f]{40}') 'Release upload action must be commit-pinned.'
  Assert-True ($workflow -match 'actions/download-artifact@[0-9a-f]{40}') 'Release download action must be commit-pinned.'
  Assert-True ($workflow -match 'contents:\s*write') 'Publish job must receive release write permission.'
  Assert-True ($workflow -match 'Refusing to overwrite existing release') 'Desktop releases must be immutable.'
  Assert-True ($workflow -match 'gh release edit' -and $workflow -match '--draft=false') 'Desktop workflow must publish only after all assets are uploaded.'
  Assert-True ($workflow -match 'SHA-256 checksums' -and $workflow -match 'not Authenticode-signed') 'Release notes must disclose checksums and signing status.'
  Assert-True ($ciWorkflow -match 'desktop-release\.Tests\.ps1') 'CI must run the desktop release contract tests.'

  Write-Output 'desktop release contract tests passed'
} finally {
  $resolvedTemporaryRoot = [IO.Path]::GetFullPath($temporaryRoot)
  if (-not $resolvedTemporaryRoot.StartsWith($temporaryBase, [StringComparison]::OrdinalIgnoreCase)) {
    throw "Refusing to clean unexpected test path: $resolvedTemporaryRoot"
  }
  if (Test-Path -LiteralPath $resolvedTemporaryRoot) {
    Remove-Item -Recurse -Force -LiteralPath $resolvedTemporaryRoot
  }
}
