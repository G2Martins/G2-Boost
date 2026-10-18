import { expect, open, t, test } from "./fixtures";

test.describe("reduced motion", () => {
  test.use({ colorScheme: "dark", reducedMotion: "reduce" });

  test("nothing animates and everything is visible at once", async ({ page }) => {
    await open(page, "boost", t["boost.title"]);
    const rising = page.locator(".animate-rise");
    await expect(rising.first()).toBeVisible();
    const states = await rising.evaluateAll((els) =>
      els.map((e) => ({ name: getComputedStyle(e).animationName, opacity: getComputedStyle(e).opacity })),
    );
    expect(states.length).toBeGreaterThan(3);
    for (const s of states) expect(s).toEqual({ name: "none", opacity: "1" });
    const route = await page.locator(".route-enter").evaluate((e) => getComputedStyle(e).animationName);
    expect(route).toBe("none");
  });
});

test.describe("normal motion", () => {
  test.use({ reducedMotion: "no-preference" });

  test("cards rise in and settle fully visible (rest state is visible)", async ({ page }) => {
    await open(page, "boost", t["boost.title"]);
    const first = page.locator(".animate-rise").first();
    expect(await first.evaluate((e) => getComputedStyle(e).animationName)).toBe("rise");
    await expect(first).toHaveCSS("opacity", "1", { timeout: 3000 });
  });
});
