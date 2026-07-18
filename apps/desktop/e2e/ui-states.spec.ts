import { expect, test } from "@playwright/test";

export const mockStates = [
  "no-model", "loading", "empty", "ready", "streaming", "cancelled", "error",
  "multi", "settings", "reset-confirm", "reload-confirm", "pack-install",
  "diagnostics", "interrupted",
] as const;

const landmarks: Record<(typeof mockStates)[number], string> = {
  "no-model": "선택한 모델이 없습니다",
  loading: "모델 로딩 중",
  empty: "새 대화를 시작하세요",
  ready: "GGUF 양자화 비교",
  streaming: "생성 중",
  cancelled: "생성을 중지했습니다",
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
