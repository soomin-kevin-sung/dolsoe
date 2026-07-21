# local-llm-wiki

Local notes and experiments for working with local LLMs.

## Desktop development

```powershell
npm --prefix apps/desktop ci
npm --prefix apps/desktop run desktop:dev
```

The first run builds and verifies the managed CPU runtime pack. Later runs reuse it while its manifest and file hashes match the pinned runtime baseline.

Release installers keep the CPU runtime bundled for offline startup and recovery. If both the installed CPU runtime and bundled recovery copy are unavailable, the desktop shell remains usable and Settings can download the checksum-pinned CPU pack from the configured runtime release; the recovered backend becomes active after restart.
