# Desktop UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the default Tauri screen with the approved Korean Local LLM Wiki desktop interface and reproduce all fourteen documented mock states without connecting native inference.

**Architecture:** A typed `RuntimeService` boundary isolates React from future Tauri commands and events. A deterministic mock service supplies fixtures selected through `?state=` while React owns transient dialogs, theme, draft, selection, and keyboard focus. Components follow `claude-ui-spec.md`; CSS tokens and layout rules remain centralized.

**Tech Stack:** Tauri 2, React 19, TypeScript, Vite, lucide-react, Playwright, CSS custom properties.

---

### Task 1: Mock State Contract And Browser Harness

**Files:**
- Modify: `apps/desktop/package.json`
- Modify: `apps/desktop/package-lock.json`
- Create: `apps/desktop/playwright.config.ts`
- Create: `apps/desktop/e2e/ui-states.spec.ts`
- Create: `apps/desktop/src/services/runtime.ts`
- Create: `apps/desktop/src/services/mockRuntime.ts`

- [ ] **Step 1: Write the failing state test**

Iterate this exact list and require `[data-app-state="<state>"]`, the Korean app name, and the state-specific landmark:

```ts
export const mockStates = [
  "no-model", "loading", "empty", "ready", "streaming", "cancelled", "error",
  "multi", "settings", "reset-confirm", "reload-confirm", "pack-install",
  "diagnostics", "interrupted",
] as const;
```

Also test `theme=dark`, 1024x700 without horizontal overflow, an 80-character model name, and `Ctrl+N`, `Ctrl+,`, `Escape`.

- [ ] **Step 2: Install test dependencies and verify RED**

```powershell
npm --prefix apps/desktop install lucide-react
npm --prefix apps/desktop install --save-dev @playwright/test
npx --prefix apps/desktop playwright install chromium
npm --prefix apps/desktop run test:e2e
```

Expected: FAIL because the scaffold has no state landmarks.

- [ ] **Step 3: Define the service boundary**

Define `MockStateName`, `ThemePreference`, `Session`, `Message`, `RuntimeTelemetry`, `RuntimePack`, and `RuntimeSnapshot`, plus:

```ts
export interface RuntimeService {
  getSnapshot(state: MockStateName): RuntimeSnapshot;
  subscribe(listener: (snapshot: RuntimeSnapshot) => void): () => void;
  sendPrompt(sessionId: string, prompt: string): Promise<void>;
  cancel(sessionId: string): Promise<void>;
  resetSession(sessionId: string): Promise<void>;
}
```

Use exact Korean copy from `docs/design/mockups/mockup.html`. Unknown query states fall back to `ready`.

- [ ] **Step 4: Add `test:e2e` scripts and verify TypeScript**

Run `npm --prefix apps/desktop run build`. Expected: PASS.

- [ ] **Step 5: Commit**

```powershell
git add apps/desktop/package.json apps/desktop/package-lock.json apps/desktop/playwright.config.ts apps/desktop/e2e apps/desktop/src/services
git commit -m "test: define desktop UI state contract"
```

### Task 2: Application Shell And Conversation Components

**Files:**
- Replace: `apps/desktop/src/App.tsx`
- Replace: `apps/desktop/src/App.css`
- Modify: `apps/desktop/src/main.tsx`
- Create: `apps/desktop/src/styles/tokens.css`
- Create: `apps/desktop/src/components/IconButton.tsx`
- Create: `apps/desktop/src/components/Sidebar.tsx`
- Create: `apps/desktop/src/components/ChatHeader.tsx`
- Create: `apps/desktop/src/components/MessageList.tsx`
- Create: `apps/desktop/src/components/Composer.tsx`
- Create: `apps/desktop/src/components/StatusBar.tsx`

- [ ] **Step 1: Extend tests for semantic landmarks**

Require `nav[aria-label="대화 목록"]`, `header`, `main[aria-label="대화"]`, `form[aria-label="메시지 입력"]`, and `[role="status"]`. Test Enter, Shift+Enter, long messages, and long model names.

- [ ] **Step 2: Verify RED**

Run `npm --prefix apps/desktop run test:e2e -- --grep "application shell"`. Expected: FAIL.

- [ ] **Step 3: Implement tokens and shell**

Copy section 2 tokens exactly. Build the 264px sidebar, 48px header, centered 760px message column, composer, and 28px status bar. Components reference CSS color variables only.

- [ ] **Step 4: Implement conversation rendering**

Add user/assistant messages, metrics, streaming cursor, cancelled/interrupted labels, model loading, empty states, and inline errors with exact mockup copy. Use section 8.2 Lucide icons with `aria-label` and `title`.

- [ ] **Step 5: Verify GREEN and commit**

```powershell
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:e2e -- --grep "application shell|long content"
git add apps/desktop/src apps/desktop/e2e
git commit -m "feat: build desktop conversation shell"
```

### Task 3: Settings, Runtime Packs, Diagnostics, And Dialogs

**Files:**
- Create: `apps/desktop/src/components/SettingsPanel.tsx`
- Create: `apps/desktop/src/components/SegmentedControl.tsx`
- Create: `apps/desktop/src/components/OptionRow.tsx`
- Create: `apps/desktop/src/components/PackRow.tsx`
- Create: `apps/desktop/src/components/DiagnosticsView.tsx`
- Create: `apps/desktop/src/components/ConfirmDialog.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/App.css`
- Modify: `apps/desktop/e2e/ui-states.spec.ts`

- [ ] **Step 1: Add failing interaction tests**

Test `Ctrl+,`, light/dark/system theme selection, diagnostics hiding the composer, reset/reload `role="dialog"`, and Escape priority: dialog, stream cancellation, overlay panel.

- [ ] **Step 2: Verify RED**

Run `npm --prefix apps/desktop run test:e2e -- --grep "settings|dialog|diagnostics|Escape"`. Expected: FAIL.

- [ ] **Step 3: Implement settings and packs**

Implement model, runtime, inference, scheduler, and screen sections. Clamp documented numeric ranges. CPU/CUDA/Vulkan use a segmented control; install progress comes from mock state. Do not add native commands or downloads.

- [ ] **Step 4: Implement diagnostics and dialogs**

Render mock ABI/runtime/device/queue rows. Dialogs return focus to their trigger; only confirmed reset uses destructive styling.

- [ ] **Step 5: Verify GREEN and commit**

```powershell
npm --prefix apps/desktop run test:e2e -- --grep "settings|dialog|diagnostics|Escape"
git add apps/desktop/src apps/desktop/e2e
git commit -m "feat: add runtime settings and diagnostics views"
```

### Task 4: Responsive, Theme, Accessibility, And Full State Coverage

**Files:**
- Modify: `apps/desktop/src/App.css`
- Modify: `apps/desktop/src/styles/tokens.css`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/e2e/ui-states.spec.ts`

- [ ] **Step 1: Add failing quality assertions**

At 1440px require a docked 320px panel; below 1280px require overlay; below 1180px require a 224px sidebar and hidden status metrics. Assert focus outlines and reduced-motion disabling spinner, pulse, cursor, and panel transitions.

- [ ] **Step 2: Verify RED**

Run `npm --prefix apps/desktop run test:e2e -- --grep "responsive|focus|reduced motion"`. Expected: FAIL.

- [ ] **Step 3: Implement quality rules**

Implement section 5 breakpoints, word wrapping, tabular numerics, focus rings, tooltips, and `prefers-reduced-motion`. Use the OS theme on first load unless a query theme is present.

- [ ] **Step 4: Verify all states and commit**

```powershell
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:e2e
git add apps/desktop/src apps/desktop/e2e
git commit -m "feat: complete responsive desktop UI states"
```

### Task 5: Product Window Metadata And Icons

**Files:**
- Modify: `apps/desktop/src-tauri/tauri.conf.json`
- Replace generated files: `apps/desktop/src-tauri/icons/*`

- [ ] **Step 1: Verify the existing config fails the approved contract**

Use PowerShell JSON parsing to assert title `Local LLM Wiki`, 1440x900, minimum 1024x700, and configured icon files. Expected: FAIL on scaffold title/minimum size.

- [ ] **Step 2: Generate identity assets**

```powershell
npm --prefix apps/desktop run tauri -- icon ../../design/app-icon-1024.png
```

Update only the documented window fields.

- [ ] **Step 3: Verify and commit**

```powershell
npm --prefix apps/desktop run build
cargo check -p dolsoe-desktop
git add apps/desktop/src-tauri
git commit -m "feat: apply desktop product identity"
```

### Task 6: Visual Verification And Milestone Review

**Files:**
- Modify only files required by verified defects

- [ ] **Step 1: Start the Vite server**

Run `npm --prefix apps/desktop run dev -- --host 127.0.0.1` and keep it running.

- [ ] **Step 2: Capture representative screenshots**

Capture ready light/dark and settings at 1440x900, plus ready/settings at 1024x700. Compare layout, copy, colors, and visibility with `docs/design/mockups/*.png`; pixel-perfect matching is not required.

- [ ] **Step 3: Perform one milestone review**

Review the complete diff against `claude-ui-spec.md`, reporting only user-visible gaps, broken interactions, accessibility failures, and responsive overflow. Fix findings in one batch without per-component loops.

- [ ] **Step 4: Run fresh final verification**

```powershell
npm --prefix apps/desktop run build
npm --prefix apps/desktop run test:e2e
cargo test -p dolsoe-desktop runtime_probe
cargo check -p dolsoe-desktop
git diff --check
```

Expected: every command passes, screenshots are nonblank, and status contains only intentional changes.

- [ ] **Step 5: Commit verified corrections if needed**

```powershell
git add apps/desktop
git commit -m "fix: align desktop UI with approved design"
```
