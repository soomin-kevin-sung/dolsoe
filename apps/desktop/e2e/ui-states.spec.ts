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
    await expect(page.getByText("돌쇠", { exact: true })).toBeVisible();
    await expect(page.getByText(landmarks[state], { exact: false }).filter({ visible: true }).first()).toBeVisible();
  });
}

test("dark theme follows the query contract", async ({ page }) => {
  await page.goto("/?state=ready&theme=dark");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await expect(page.locator(".app-mark path")).toHaveCSS("fill", "rgb(255, 255, 255)");
});

test("theme keeps action and ready colors distinct", async ({ page }) => {
  await page.goto("/?state=ready&theme=light");
  await expect(page.locator(".status-dot.ready").first()).toHaveCSS("background-color", "rgb(110, 158, 0)");
  expect(await page.locator("html").evaluate((element) => getComputedStyle(element).getPropertyValue("--accent").trim()))
    .toBe("#7057f5");

  await page.goto("/?state=ready&theme=dark");
  await expect(page.locator(".status-dot.ready").first()).toHaveCSS("background-color", "rgb(196, 239, 77)");
  expect(await page.locator("html").evaluate((element) => getComputedStyle(element).getPropertyValue("--accent").trim()))
    .toBe("#927fff");
});

test("bundled static Wanted Sans weights are available for Korean UI text", async ({ page }) => {
  await page.goto("/?state=ready&theme=light");
  await page.evaluate(() => document.fonts.ready);

  await expect(page.locator("body")).toHaveCSS("font-family", /Wanted Sans/);
  expect(await page.evaluate(async () => {
    const faces = await Promise.all([400, 500, 600, 700].map((weight) => document.fonts.load(`${weight} 14px "Wanted Sans"`, "로컬 대화")));
    return faces.every((loaded) => loaded.length > 0);
  })).toBe(true);
});

test("runtime summary makes state and detail readable", async ({ page }) => {
  await page.goto("/?state=ready&theme=light");
  const readySummary = page.getByRole("button", { name: "로컬 AI 상태: 준비됨" });
  await expect(readySummary.locator(".runtime-state-icon.ready")).toBeVisible();
  await expect(readySummary.locator(".lucide-layers")).toBeVisible();
  await expect(readySummary.locator("strong")).toHaveCSS("font-size", "13px");
  await expect(readySummary.locator("small")).toHaveCSS("font-size", "11px");
  await expect(readySummary.locator("small")).toContainText("Qwen2.5");

  await page.goto("/?state=loading");
  await expect(page.locator(".runtime-state-icon.loading .spin")).toBeVisible();

  await page.goto("/?state=error");
  await expect(page.locator(".runtime-state-icon.error")).toBeVisible();

  await page.goto("/?state=no-model");
  await expect(page.getByText("사용할 GGUF 모델을 선택하세요")).toBeVisible();
  await expect(page.locator(".runtime-summary-button .lucide-box")).toBeVisible();
});

test("calm home starts a new local conversation", async ({ page }) => {
  await page.goto("/?state=ready&view=home&theme=light");
  await expect(page.getByRole("main", { name: "홈" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "무엇을 함께 정리할까요?" })).toBeVisible();
  const firstMessage = page.getByRole("textbox", { name: "첫 메시지" });
  await firstMessage.fill("새 대화 테스트");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("main", { name: "대화" })).toBeVisible();
  await expect(page.locator('[data-message-role="user"]').last()).toContainText("새 대화 테스트");
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

test("header keeps model selection in the home and sidebar flows", async ({ page }) => {
  await page.goto("/?state=no-model&view=home");

  await expect(page.locator("header").getByRole("button", { name: /GGUF 모델 선택/ })).toHaveCount(0);
  await expect(page.getByRole("main", { name: "홈" }).getByRole("button", { name: "모델 선택" })).toBeVisible();
  await expect(page.locator(".home-setup-steps strong").first()).toHaveCSS("font-size", "13px");
  await expect(page.locator(".home-setup-steps small").first()).toHaveCSS("font-size", "11px");
  await expect(page.locator(".home-setup-copy h2")).toHaveCSS("font-size", "14px");
  await expect(page.locator(".home-setup-copy p")).toHaveCSS("font-size", "13px");
  const runtimeSummary = page.locator(".runtime-summary-button");
  await expect(runtimeSummary).toBeVisible();
  await runtimeSummary.click();
  const modelMenu = page.getByRole("dialog", { name: "모델 관리" });
  await expect(modelMenu).toBeVisible();
  await expect(modelMenu.getByRole("button", { name: "모델 선택" })).toBeVisible();
});

test("global keyboard shortcuts change application state", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.keyboard.press("Control+n");
  await expect(page.locator('[data-app-state="empty"]')).toBeVisible();
  await page.keyboard.press("Control+,");
  await expect(page.getByRole("dialog", { name: "설정" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "설정" })).toHaveCount(0);
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

test("conversation follows the latest message after a new prompt", async ({ page }) => {
  await page.setViewportSize({ width: 1024, height: 700 });
  await page.goto("/?state=ready&longMessage=1");
  const conversation = page.getByRole("main", { name: "대화" });
  const remainingScroll = () => conversation.evaluate(
    (element) => element.scrollHeight - element.scrollTop - element.clientHeight,
  );

  await expect.poll(remainingScroll).toBeLessThanOrEqual(1);
  await conversation.evaluate((element) => { element.scrollTop = 0; });
  expect(await remainingScroll()).toBeGreaterThan(96);

  await page.getByRole("textbox", { name: "메시지" }).fill("자동 스크롤 확인");
  await page.keyboard.press("Enter");

  await expect.poll(remainingScroll).toBeLessThanOrEqual(1);
  await expect(page.locator('[data-message-role="user"]').last()).toContainText("자동 스크롤 확인");

  await conversation.evaluate((element) => { element.scrollTop = 0; });
  await page.getByRole("button", { name: "생성 중지" }).click();
  await expect(conversation).toHaveJSProperty("scrollTop", 0);
});

test("settings opens from keyboard and changes theme", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.keyboard.press("Control+,");
  const settings = page.getByRole("dialog", { name: "설정" });
  await expect(settings).toBeVisible();
  await expect(settings.getByRole("tab", { name: "일반" })).toHaveAttribute("aria-selected", "true");
  await expect(settings.getByRole("tab", { name: "모델" })).toHaveCount(0);
  await settings.getByRole("button", { name: "다크" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await settings.getByRole("button", { name: "라이트" }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await expect(settings.getByRole("button", { name: "시스템" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(settings).toHaveCount(0);
});

test("general startup preferences persist across reloads", async ({ page }) => {
  await page.goto("/?state=settings");
  const settings = page.getByRole("dialog", { name: "설정" });
  await expect(settings.getByRole("button", { name: "홈", exact: true })).toHaveAttribute("aria-pressed", "true");
  await settings.getByRole("button", { name: "마지막 대화" }).click();
  await settings.getByRole("checkbox", { name: "마지막 모델 자동 로드" }).check();

  await page.reload();
  const reopened = page.getByRole("dialog", { name: "설정" });
  await expect(reopened.getByRole("button", { name: "마지막 대화" })).toHaveAttribute("aria-pressed", "true");
  await expect(reopened.getByRole("checkbox", { name: "마지막 모델 자동 로드" })).toBeChecked();
});

test("advanced generation options accept decimal values and stop sequences", async ({ page }) => {
  await page.goto("/?state=settings");
  const settings = page.getByRole("dialog", { name: "설정" });
  await settings.getByRole("tab", { name: "생성" }).click();
  const maxTokens = settings.getByRole("slider", { name: "최대 생성 토큰" });
  await expect(maxTokens).toHaveAttribute("aria-valuetext", "256 토큰");
  await maxTokens.press("End");
  await expect(maxTokens).toHaveAttribute("aria-valuetext", "8,192 토큰");

  const randomSeed = settings.getByRole("checkbox", { name: "매번 무작위" });
  const seed = settings.getByRole("textbox", { name: "고정 Seed" });
  await expect(randomSeed).toBeChecked();
  await expect(seed).toBeDisabled();
  await expect(settings.getByText("응답마다 무작위", { exact: true })).toBeVisible();
  await randomSeed.uncheck();
  await expect(seed).toBeEnabled();
  await seed.fill("42");
  await seed.press("Tab");
  await expect(settings.getByText("시드 42 사용", { exact: true })).toBeVisible();

  await expect(settings.getByRole("combobox", { name: "Top-K" })).toHaveValue("40");
  const minP = settings.getByRole("textbox", { name: "Min-P 정확한 값" });
  await minP.fill("0.12");
  await minP.press("Tab");
  await expect(minP).toHaveValue("0.12");

  await settings.getByRole("textbox", { name: "중지 문자열" }).fill("<END>");
  await settings.getByRole("button", { name: "중지 문자열 추가" }).click();
  await expect(settings.getByText("<END>", { exact: true })).toBeVisible();
  await settings.getByRole("button", { name: "<END> 제거" }).click();
  await expect(settings.getByText("<END>", { exact: true })).toHaveCount(0);

  await expect(settings.getByText("다음 응답", { exact: true }).first()).toBeVisible();
  await settings.getByRole("tab", { name: "성능" }).click();
  await expect(settings.getByRole("button", { name: "모델 다시 로드" })).toHaveCount(0);
  const contextSize = settings.getByRole("slider", { name: "컨텍스트 길이" });
  await expect(contextSize).toHaveAttribute("aria-valuetext", "4,096 토큰");
  await contextSize.press("End");
  await expect(contextSize).toHaveAttribute("aria-valuetext", "131,072 토큰");
  await expect(settings.getByText("현재 4,096 토큰 → 변경 131,072 토큰")).toBeVisible();
  await expect(settings.getByRole("button", { name: "모델 다시 로드" })).toBeVisible();
  await expect(settings.getByRole("slider", { name: "배치 크기", exact: true })).toBeVisible();
  await expect(settings.getByRole("slider", { name: "물리 배치 크기", exact: true })).toBeVisible();
  const markOffsets = await settings.evaluate((dialog) => Array.from(dialog.querySelectorAll(".inference-option"))
    .map((row) => {
      const slider = row.querySelector(".option-slider.discrete");
      const marks = Array.from(row.querySelectorAll(".slider-marks span"));
      if (!slider || marks.length < 2) return [];
      const sliderRect = slider.getBoundingClientRect();
      return marks.map((mark, index) => {
        const markRect = mark.getBoundingClientRect();
        const markCenter = markRect.left + markRect.width / 2;
        const stopCenter = sliderRect.left + 8 + index * ((sliderRect.width - 16) / (marks.length - 1));
        return Math.abs(markCenter - stopCenter);
      });
    })
    .flat());
  expect(Math.max(...markOffsets)).toBeLessThan(0.1);
  await expect(settings.getByRole("checkbox", { name: "메모리 매핑" })).toBeChecked();
  await expect(settings.getByText("모델 재로드", { exact: true })).toBeVisible();
});

test("settings number fields use hover fill and focus underline", async ({ page }) => {
  await page.goto("/?state=settings");
  await page.getByRole("tab", { name: "생성" }).click();
  const temperature = page.getByRole("textbox", { name: "Temperature 정확한 값" });

  await temperature.hover();
  await expect(temperature).toHaveCSS("background-color", "rgb(236, 233, 248)");
  await temperature.focus();
  await expect(temperature).toHaveCSS("outline-style", "none");
  await expect(temperature).toHaveCSS("box-shadow", /rgb\(112, 87, 245\)/);
});

test("runtime control disables unavailable backends", async ({ page }) => {
  await page.goto("/?state=settings");
  await page.getByRole("tab", { name: "런타임" }).click();
  await expect(page.getByRole("button", { name: "Vulkan" })).toBeDisabled();
  await expect(page.getByText("Vulkan 런타임이 설치되어 있지 않습니다.")).toBeVisible();
});

test("runtime selection exposes pending reload state", async ({ page }) => {
  await page.goto("/?state=settings");
  await page.getByRole("tab", { name: "런타임" }).click();
  await page.getByRole("button", { name: "CUDA", exact: true }).click();
  await expect(page.getByText("변경됨")).toBeVisible();
  await expect(page.getByRole("button", { name: "모델 다시 로드" })).toBeVisible();

  await page.getByRole("button", { name: "설정 닫기" }).click();
  await page.keyboard.press("Control+,");
  await page.getByRole("tab", { name: "런타임" }).click();
  await expect(page.getByRole("button", { name: "CPU", exact: true })).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByRole("button", { name: "모델 다시 로드" })).toHaveCount(0);
});

test("model menu stages a replacement until explicit confirmation", async ({ page }) => {
  await page.goto("/?state=ready");
  const summary = page.getByRole("button", { name: "로컬 AI 상태: 준비됨" });
  await summary.click();
  const menu = page.getByRole("dialog", { name: "모델 관리" });
  await menu.getByRole("button", { name: "다른 모델 선택" }).click();

  await expect(menu.getByText("Qwen2.5-7B-Instruct-Q4_K_M.gguf", { exact: true })).toBeVisible();
  await expect(menu.getByText("Llama-3.1-8B-Instruct-Q4_K_M.gguf", { exact: true })).toBeVisible();
  await expect(menu.getByRole("button", { name: "이 모델로 교체" })).toBeVisible();
  await expect(summary.locator("small")).toContainText("Qwen2.5");

  await menu.getByRole("button", { name: "취소" }).click();
  await expect(menu.getByRole("button", { name: "다른 모델 선택" })).toBeVisible();
  await expect(menu.getByText("Llama-3.1-8B-Instruct-Q4_K_M.gguf", { exact: true })).toHaveCount(0);

  await menu.getByRole("button", { name: "다른 모델 선택" }).click();
  await menu.getByRole("button", { name: "이 모델로 교체" }).click();
  await expect(menu).toHaveCount(0);
  await expect(summary.locator("small")).toContainText("Llama-3.1");
});

test("model menu opens runtime settings directly", async ({ page }) => {
  await page.goto("/?state=ready");
  await page.getByRole("button", { name: "로컬 AI 상태: 준비됨" }).click();
  await page.getByRole("dialog", { name: "모델 관리" }).getByRole("button", { name: "런타임 설정" }).click();

  const settings = page.getByRole("dialog", { name: "설정" });
  await expect(settings.getByRole("tab", { name: "런타임" })).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("dialog", { name: "모델 관리" })).toHaveCount(0);
});

test("runtime pack installation shows progress and locks competing installs", async ({ page }) => {
  await page.goto("/?state=pack-install");

  const cudaPack = page.locator(".pack-row").filter({ hasText: "CUDA" });
  await expect(cudaPack).toContainText("64%");
  await expect(cudaPack.locator(".progress-fill")).toHaveAttribute("style", /64%/);

  const vulkanPack = page.locator(".pack-row").filter({ hasText: "Vulkan" });
  await expect(vulkanPack.getByRole("button", { name: "설치" })).toBeDisabled();
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
  await expect(page.getByRole("dialog", { name: "모델을 다시 로드할까요?" })).toHaveCount(0);
  await expect(page.getByRole("dialog", { name: "설정" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("dialog", { name: "설정" })).toHaveCount(0);
  await page.goto("/?state=streaming");
  await page.keyboard.press("Escape");
  await expect(page.locator('[data-app-state="cancelled"]')).toBeVisible();
});

test("responsive settings dialog stays centered and inside the viewport", async ({ page }) => {
  await page.setViewportSize({ width: 1440, height: 900 });
  await page.goto("/?state=settings");
  const panel = page.getByRole("dialog", { name: "설정" });
  await expect(panel).toHaveCSS("width", "860px");
  await expect(panel).toHaveCSS("position", "static");
  const desktopBox = await panel.boundingBox();
  expect(desktopBox).not.toBeNull();
  expect(Math.abs((desktopBox?.x ?? 0) + (desktopBox?.width ?? 0) / 2 - 720)).toBeLessThan(2);

  await page.setViewportSize({ width: 640, height: 700 });
  await expect(panel).toHaveCSS("width", "616px");
  const mobileBox = await panel.boundingBox();
  expect((mobileBox?.height ?? 701)).toBeLessThanOrEqual(650);
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(640);
});

test("settings dialog keeps a stable frame across tabs", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.emulateMedia({ reducedMotion: "reduce" });
  await page.goto("/?state=settings");
  const panel = page.locator(".settings-panel");
  const tabs = page.locator(".settings-tabs button");
  await expect(tabs).toHaveCount(4);
  const initialBox = await panel.boundingBox();
  expect(initialBox).not.toBeNull();

  for (let index = 0; index < 4; index += 1) {
    await tabs.nth(index).click();
    const nextBox = await panel.boundingBox();
    expect(nextBox?.width).toBe(initialBox?.width);
    expect(nextBox?.height).toBe(initialBox?.height);
  }
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
  await expect(page.locator(".runtime-state-icon")).toHaveCSS("animation-name", "none");
  await expect(page.locator(".runtime-summary-copy")).toHaveCSS("animation-name", "none");
  await page.keyboard.press("Control+,");
  await expect(page.getByRole("dialog", { name: "설정" })).toHaveCSS("animation-name", "none");
});

test("stop icon is centered in its button", async ({ page }) => {
  await page.goto("/?state=streaming");
  const alignment = await page.locator(".stop-button").evaluate((button) => {
    const icon = button.querySelector(".stop-button-icon");
    if (!icon) return null;
    const buttonRect = button.getBoundingClientRect();
    const iconRect = icon.getBoundingClientRect();
    return {
      x: Math.abs((buttonRect.left + buttonRect.width / 2) - (iconRect.left + iconRect.width / 2)),
      y: Math.abs((buttonRect.top + buttonRect.height / 2) - (iconRect.top + iconRect.height / 2)),
    };
  });
  expect(alignment).not.toBeNull();
  expect(alignment?.x).toBeLessThan(0.1);
  expect(alignment?.y).toBeLessThan(0.1);
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
