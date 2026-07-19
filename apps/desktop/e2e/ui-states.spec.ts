import { expect, test } from "@playwright/test";

export const mockStates = [
  "no-model", "loading", "empty", "ready", "streaming", "cancelled", "error",
  "multi", "settings", "reset-confirm", "reload-confirm", "pack-install",
  "diagnostics", "interrupted",
] as const;

const landmarks: Record<(typeof mockStates)[number], string> = {
  "no-model": "선택된 모델이 없습니다",
  loading: "모델 로딩 중",
  empty: "새 대화를 시작하세요",
  ready: "GGUF 양자화 비교",
  streaming: "생성 중",
  cancelled: "생성이 중지되었습니다",
  error: "CUDA 백엔드를 초기화하지 못했습니다",
  multi: "생성 중 · 2",
  settings: "설정",
  "reset-confirm": "대화를 초기화할까요?",
  "reload-confirm": "모델을 다시 로드할까요?",
  "pack-install": "설치 중",
  diagnostics: "진단",
  interrupted: "생성이 중단되었습니다",
};

for (const state of mockStates) {
  test(`mock state: ${state}`, async ({ page }) => {
    await page.goto(`/?state=${state}`);
    await expect(page.locator(`[data-app-state="${state}"]`)).toBeVisible();
    await expect(page.getByText("Local LLM Wiki", { exact: true })).toBeVisible();
    await expect(page.getByText(landmarks[state], { exact: false }).first()).toBeVisible();
  });
}

test("dark theme follows the query contract", async ({ page }) => {
  await page.goto("/?state=ready&theme=dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});

test("minimum window has no horizontal overflow", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.goto("/?state=ready&longModel=1");
  const overflow = await page.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  expect(overflow).toBeLessThanOrEqual(0);
  await expect(page.locator("[data-model-name]").first()).toContainText("Q".repeat(80));
});

test("global keyboard shortcuts change application state", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.keyboard.press("Control+n");
  await expect(page.locator('[data-app-state="empty"]')).toBeVisible();
  await page.keyboard.press("Control+,");
  await expect(page.getByRole("complementary", { name: "설정" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("complementary", { name: "설정" })).toBeHidden();
});

test("application shell exposes semantic landmarks", async ({ page }) => {
  await page.goto("/?state=ready");
  await expect(page.getByRole("navigation", { name: "대화 목록" })).toBeVisible();
  await expect(page.locator("header")).toBeVisible();
  await expect(page.getByRole("main", { name: "대화" })).toBeVisible();
  await expect(page.getByRole("form", { name: "메시지 입력" })).toBeVisible();
  await expect(page.getByRole("status")).toBeVisible();
});

test("application shell handles Enter and Shift+Enter", async ({ page }) => {
  await page.goto("/?state=empty");
  const input = page.getByRole("textbox", { name: "메시지" });
  await input.fill("첫째 줄");
  await page.keyboard.press("Shift+Enter");
  await input.type("둘째 줄");
  await expect(input).toHaveValue("첫째 줄\n둘째 줄");
  await page.keyboard.press("Enter");
  await expect(page.locator('[data-message-role="user"]').last()).toContainText("첫째 줄");
});

test("long content stays inside the application shell", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.goto("/?state=ready&longModel=1&longMessage=1");
  await expect(page.locator("[data-model-name]").first()).toContainText("Q".repeat(80));
  await expect(page.locator("[data-long-message]")).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(1024);
});

test("settings opens from keyboard and changes theme", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("complementary", { name: "설정" });
  await expect(settings).toBeVisible();
  await settings.getByRole("button", { name: "다크" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await settings.getByRole("button", { name: "라이트" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(settings.getByRole("button", { name: "시스템" })).toBeVisible();
});

test("runtime control disables unavailable backends", async ({ page }) => {
  await page.goto("/?state=settings");
  await expect(page.getByRole("button", { name: "Vulkan" })).toBeDisabled();
  await expect(page.getByText("Vulkan 런타임이 설치되어 있지 않습니다.")).toBeVisible();
});

test("runtime selection exposes pending reload state", async ({ page }) => {
  await page.goto("/?state=settings");
  await page.getByRole("button", { name: "CUDA", exact: true }).click();
  await expect(page.getByText("재로드 대기")).toBeVisible();
  await expect(page.getByRole("button", { name: "적용하고 모델 다시 로드" })).toBeVisible();
});

test("diagnostics replaces the conversation composer", async ({ page }) => {
  await page.goto("/?state=diagnostics");
  await expect(page.getByRole("heading", { name: "진단" })).toBeVisible();
  await expect(page.getByRole("form", { name: "메시지 입력" })).toHaveCount(0);
  await expect(page.getByText("Bridge ABI", { exact: true })).toBeVisible();
});

test("dialog states expose the approved confirmations", async ({ page }) => {
  await page.goto("/?state=reset-confirm");
  await expect(page.getByRole("dialog", { name: "대화를 초기화할까요?" })).toBeVisible();
  await expect(page.getByRole("button", { name: "초기화", exact: true })).toBeVisible();
  await page.goto("/?state=reload-confirm");
  await expect(page.getByRole("dialog", { name: "모델을 다시 로드할까요?" })).toBeVisible();
  await expect(page.getByRole("button", { name: "다시 로드", exact: true })).toBeVisible();
});

test("Escape closes dialog before panel and cancels streaming", async ({ page }) => {
  await page.goto("/?state=reload-confirm");
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.getByRole("complementary", { name: "설정" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("complementary", { name: "설정" })).toBeHidden();
  await page.goto("/?state=streaming");
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-app-state="cancelled"]')).toBeVisible();
});

test("responsive settings panel docks and overlays at approved breakpoints", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?state=settings");
  const panel = page.getByRole("complementary", { name: "설정" });
  await expect(panel).toHaveCSS("width", "320px");
  await expect(panel).toHaveCSS("position", "static");
  await page.setViewportSize({ width: 1279, height: 800 });
  await expect(panel).toHaveCSS("position", "fixed");
  await expect(panel).toHaveCSS("right", "0px");
  await page.setViewportSize({ width: 1179, height: 700 });
  await expect(page.getByRole("navigation", { name: "대화 목록" })).toHaveCSS("width", "224px");
  await expect(page.locator(".metric-토큰")).toBeHidden();
  await expect(page.locator(".metric-시간")).toBeHidden();
});

test("focus styling is visible for keyboard navigation", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.keyboard.press("Tab");
  const focused = page.locator(":focus");
  await expect(focused).toBeVisible();
  expect(await focused.evaluate((element) => getComputedStyle(element).outlineStyle)).not.toBe("none");
});

test("reduced motion disables cursor pulse spinner and panel transition", async ({ page }) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/?state=streaming");
  await expect(page.locator(".streaming-cursor")).toHaveCSS("animation-name", "none");
  await expect(page.locator(".status-dot.streaming")).toHaveCSS("animation-name", "none");
  await expect(page.locator(".spin").first()).toHaveCSS("animation-name", "none");
  await page.keyboard.press("Control+,");
  await expect(page.getByRole("complementary", { name: "설정" })).toHaveCSS("transition-duration", "0s");
});

test("system theme follows the operating system on first load", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await page.goto("/?state=ready");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
});

test("keyboard conversation flow sends then stops generation", async ({ page }) => {
  await page.goto("/?state=empty&theme=light");
  const input = page.getByRole("textbox", { name: "메시지" });
  await input.fill("테스트 질문");
  await page.keyboard.press("Enter");
  await expect(page.locator('[data-app-state="streaming"]')).toBeVisible();
  await page.getByRole("button", { name: "생성 중지" }).click();
  await expect(page.locator('[data-app-state="cancelled"]')).toBeVisible();
  await expect(input).toBeFocused();
});

test("session selection returns from diagnostics", async ({ page }) => {
  await page.goto("/?state=diagnostics");
  await page.locator(".session-item").filter({ hasText: "GGUF 양자화 비교" }).click();
  await expect(page.locator('[data-app-state="ready"]')).toBeVisible();
  await expect(page.getByRole("form", { name: "메시지 입력" })).toBeVisible();
});

test("multi state distinguishes queued work", async ({ page }) => {
  await page.goto("/?state=multi");
  await expect(page.locator(".session-item").nth(2)).toContainText("대기 중");
});

test("no-model telemetry contains no stale inference values", async ({ page }) => {
  await page.goto("/?state=no-model");
  const values = page.locator(".status-metric-value");
  await expect(values).toHaveCount(5);
  for (let index = 0; index < 5; index += 1) await expect(values.nth(index)).toHaveText("—");
});

test("Ctrl+F focuses search and modal focus remains trapped", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.keyboard.press("Control+f");
  await expect(page.getByRole("searchbox", { name: "대화 검색" })).toBeFocused();
  await page.goto("/?state=reset-confirm");
  const dialog = page.getByRole("dialog");
  for (let index = 0; index < 4; index += 1) {
    await page.keyboard.press("Tab");
    await expect(dialog.locator(":focus")).toHaveCount(1);
  }
});

test("approved Korean copy is preserved", async ({ page }) => {
  await page.goto("/?state=ready");
  await expect(page.getByText(/파일 크기가 약 4\.4GB/)).toBeVisible();
  await expect(page.getByText(/KV 캐시가 약 0\.9GB 더 필요합니다/)).toBeVisible();
  await expect(page.locator("svg.app-mark")).toBeVisible();
  await page.goto("/?state=no-model");
  await expect(page.getByRole("heading", { name: "선택된 모델이 없습니다" })).toBeVisible();
});

test("normal browser URL explains that the desktop app is required", async ({ page }) => {
  await page.goto("/");

  await expect(page.getByRole("heading", { name: "데스크톱 앱에서 실행해야 합니다" })).toBeVisible();
  await expect(page.getByText("npm --prefix apps/desktop run tauri -- dev")).toBeVisible();
});

test("session search filters the persisted-style list", async ({ page }) => {
  await page.goto("/?state=ready");
  const search = page.getByRole("searchbox", { name: "대화 검색" });

  await search.fill("CUDA");

  await expect(page.locator(".session-item")).toHaveCount(1);
  await expect(page.locator(".session-item")).toContainText("CUDA 오프로딩 설정 정리");
});

test("session actions expose rename clear and delete commands", async ({ page }) => {
  await page.goto("/?state=ready");

  await page.getByRole("button", { name: "GGUF 양자화 비교 대화 메뉴" }).click();

  await expect(page.getByRole("menuitem", { name: "이름 변경" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "대화 초기화" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "삭제" })).toBeVisible();
});
