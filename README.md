# local-llm-wiki

Local notes and experiments for working with local LLMs.

## Desktop development

```powershell
npm --prefix apps/desktop ci
npm --prefix apps/desktop run desktop:dev
```

The first run builds and verifies the managed CPU runtime pack. Later runs reuse it while its manifest and file hashes match the pinned runtime baseline.
