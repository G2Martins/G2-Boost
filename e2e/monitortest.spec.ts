import { expect, open, t, test } from "./fixtures";

const TESTS = [
  "tools.monitor.t.solid",
  "tools.monitor.t.blacks",
  "tools.monitor.t.whites",
  "tools.monitor.t.gradient",
  "tools.monitor.t.uniformity",
  "tools.monitor.t.sharpness",
  "tools.monitor.t.motion",
] as const;

test("the monitor test lists every test and nothing runs until you start one", async ({ page }) => {
  await open(page, "monitortest", t["nav.monitortest"]);
  for (const key of TESTS) {
    await expect(page.getByRole("button", { name: t[key] }).first()).toBeVisible();
  }
  await expect(page.getByText(t["tools.monitor.motion.notice"])).toBeVisible();
  await expect(page.getByText(t["tools.monitor.screen.rate.none"])).toBeVisible();
  // The motion test only exists while it runs.
  await expect(page.getByTestId("motion-rate")).toHaveCount(0);
  await expect(page.getByRole("application")).toHaveCount(0);
});

test("a test opens over everything, arrows move through it and Esc leaves", async ({ page }) => {
  await open(page, "monitortest", t["nav.monitortest"]);
  await page.getByRole("button", { name: t["tools.monitor.start.button"] }).click();
  const surface = page.getByRole("application", { name: t["tools.monitor.surface"] });
  await expect(surface).toBeVisible();

  // Five solid colours first: black, white, red, green, blue. Space and the arrows move through them.
  await expect(surface.getByText("Preto (1 de 5)")).toBeVisible();
  await page.keyboard.press("ArrowRight");
  await expect(surface.getByText("Branco (2 de 5)")).toBeVisible();
  await page.keyboard.press("Space");
  await expect(surface.getByText("Vermelho (3 de 5)")).toBeVisible();
  await page.keyboard.press("ArrowLeft");
  await expect(surface.getByText("Branco (2 de 5)")).toBeVisible();
  // The pattern really is that colour: the first thing under the hint.
  const fill = await surface.locator("div").first().evaluate((e) => getComputedStyle(e).backgroundColor);
  expect(fill).toBe("rgb(255, 255, 255)");

  // Up and down jump between tests: blacks, whites, ..., and back.
  await page.keyboard.press("ArrowDown");
  await expect(surface.getByText(t["tools.monitor.t.blacks"]).first()).toBeVisible();
  await page.keyboard.press("ArrowUp");
  await expect(surface.getByText("Preto (1 de 5)")).toBeVisible();
  await page.keyboard.press("ArrowUp");
  await expect(surface.getByText(t["tools.monitor.t.motion"]).first()).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(page.getByRole("application")).toHaveCount(0);
  await expect(page.getByRole("heading", { level: 1, name: t["nav.monitortest"] })).toBeVisible();
});

test("a click moves to the next pattern too", async ({ page }) => {
  await open(page, "monitortest", t["nav.monitortest"]);
  await page.getByRole("button", { name: t["tools.monitor.t.uniformity"] }).first().click();
  const surface = page.getByRole("application", { name: t["tools.monitor.surface"] });
  await expect(surface.getByText("Preto (1 de 3)")).toBeVisible();
  await surface.click({ position: { x: 200, y: 200 } });
  await expect(surface.getByText("Cinza escuro (2 de 3)")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("application")).toHaveCount(0);
});

test("the pixel patterns draw: levels, ramps and the sharpness grid", async ({ page }) => {
  await open(page, "monitortest", t["nav.monitortest"]);
  const surface = page.getByRole("application", { name: t["tools.monitor.surface"] });
  await page.getByRole("button", { name: t["tools.monitor.t.blacks"] }).first().click();
  // Eleven squares, 0 to 10.
  await expect(surface.getByText("10", { exact: true })).toBeVisible();
  await expect(surface.getByText("0", { exact: true })).toBeVisible();
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: t["tools.monitor.t.gradient"] }).first().click();
  const ramp = surface.locator("canvas");
  await expect(ramp).toBeVisible();
  // Left end is black, right end is white (the grey ramp is the top band).
  const ends = await ramp.evaluate((c: HTMLCanvasElement) => {
    const ctx = c.getContext("2d");
    if (!ctx) return null;
    const y = Math.floor(c.height / 8);
    return [Array.from(ctx.getImageData(1, y, 1, 1).data), Array.from(ctx.getImageData(c.width - 2, y, 1, 1).data)];
  });
  expect(ends?.[0]?.slice(0, 3)).toEqual([0, 0, 0]);
  expect(ends?.[1]?.slice(0, 3)).toEqual([255, 255, 255]);
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: t["tools.monitor.t.sharpness"] }).first().click();
  const grid = await surface.locator("canvas").evaluate((c: HTMLCanvasElement) => {
    const ctx = c.getContext("2d");
    if (!ctx) return null;
    // Top-left quarter is a 1 px checkerboard: neighbours alternate black and white.
    const px = (x: number, y: number) => ctx.getImageData(x, y, 1, 1).data[0];
    return { a: px(10, 10), b: px(11, 10), c: px(10, 11), frame: px(0, 4) };
  });
  expect(grid).not.toBeNull();
  expect(grid?.a).not.toBe(grid?.b);
  expect(grid?.a).not.toBe(grid?.c);
  expect(grid?.frame).toBe(255);
  await page.keyboard.press("Escape");
  await expect(page.getByRole("application")).toHaveCount(0);
});

test("the motion test measures the refresh rate while it runs and keeps the last reading", async ({
  page,
}) => {
  await open(page, "monitortest", t["nav.monitortest"]);
  await page.getByRole("button", { name: t["tools.monitor.t.motion"] }).first().click();
  const rate = page.getByTestId("motion-rate");
  await expect(rate).toBeVisible();
  await expect(rate).toContainText(/\d/, { timeout: 15_000 });
  const surface = page.getByRole("application");
  await expect(surface.getByText(t["tools.monitor.motion.dropped"], { exact: true })).toBeVisible();
  await expect(surface.getByText(t["tools.monitor.motion.lane"].replace("{speed}", "960"))).toBeVisible();
  await page.waitForTimeout(3000);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("motion-rate")).toHaveCount(0);
  // Back on the launcher the measurement is kept.
  await expect(page.getByText(t["tools.monitor.screen.rate.none"])).toHaveCount(0);
  await expect(page.getByText(/Medida no teste de movimento/)).toBeVisible();
});

test.describe("reduced motion", () => {
  test.use({ colorScheme: "dark", reducedMotion: "reduce" });

  test("the launcher shows everything at once and the hint does not fade", async ({ page }) => {
    await open(page, "monitortest", t["nav.monitortest"]);
    const rising = page.locator(".animate-rise");
    await expect(rising.first()).toBeVisible();
    const states = await rising.evaluateAll((els) =>
      els.map((e) => ({ name: getComputedStyle(e).animationName, opacity: getComputedStyle(e).opacity })),
    );
    expect(states.length).toBeGreaterThan(6);
    for (const s of states) expect(s).toEqual({ name: "none", opacity: "1" });
    await page.getByRole("button", { name: t["tools.monitor.t.solid"] }).first().click();
    const hint = page.getByRole("application").locator(".bottom-8");
    await expect(hint).toBeVisible();
    // No fade when the user asked for less motion: the hint just goes.
    expect(await hint.evaluate((e) => getComputedStyle(e).transitionDuration)).toBe("0s");
    await page.keyboard.press("Escape");
  });
});
