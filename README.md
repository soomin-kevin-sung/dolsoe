# Dolsoe

Local notes and experiments for working with local LLMs.

## Desktop development

```powershell
npm --prefix apps/desktop ci
npm --prefix apps/desktop run desktop:dev
```

The first run builds and verifies the managed CPU runtime pack. Later runs reuse it while its manifest and file hashes match the pinned runtime baseline.

Release installers keep the CPU runtime bundled for offline startup and recovery. If both the installed CPU runtime and bundled recovery copy are unavailable, the desktop shell remains usable and Settings can download the checksum-pinned CPU pack from the configured runtime release; the recovered backend becomes active after restart.

## Desktop releases

Desktop releases are immutable Windows x64 GitHub Releases built with Velopack. The pipeline validates that the Tauri, npm, and Cargo package versions match, downloads an existing runtime release, bundles its verified CPU pack, runs frontend and Rust tests, and builds a Velopack one-click installer plus full and delta update packages. Published assets include the `win-x64` update feed, `desktop-release.json`, and `SHA256SUMS.txt`.

Tauri only builds `dolsoe-desktop.exe`; it does not create an NSIS or MSI bundle. The release job stages the executable and bundled runtime, then uses the version-pinned Velopack CLI to run the `download -> pack -> upload` sequence. .NET 8 is required only in the build environment to run `vpk`. Installed copies of Dolsoe do not require .NET, and the installer bootstraps WebView2 when it is missing.

Publish the runtime packs first, then dispatch the desktop workflow:

```powershell
gh workflow run "runtime release" -f version=0.1.1
gh workflow run "desktop release" -f version=0.1.0 -f runtime_tag=runtime-v0.1.1
```

A pushed `desktop-v<version>` tag also triggers the desktop release and defaults to `runtime-v<version>`. Release versions must already match `apps/desktop/package.json`, `apps/desktop/src-tauri/Cargo.toml`, and `apps/desktop/src-tauri/tauri.conf.json`.

The current pipeline produces an unsigned `Dolsoe-win-x64-Setup.exe`. Windows Authenticode signing remains a separate release-hardening step.
