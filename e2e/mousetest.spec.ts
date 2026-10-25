import type { Page } from "@playwright/test";
import { expect, open, t, test } from "./fixtures";

/** Move through the middle of `testid` in fast circles. */
async function circles(page: Page, testid: string) {
  const box = await page.getByTestId(testid).boundingBox();
  if (!box) throw new Error(`${testid} has no box`);
  const cx = box.x + box.width / 2;
  const cy = box.y + box.height / 2;
  await page.mouse.move(cx, cy);
  for (let i = 0; i < 150; i++) {
    const a = i / 5;
    await page.mouse.move(cx + 90 * Math.cos(a), cy + 60 * Math.sin(a));
  }
  return { cx, cy };
}

test("the polling test reads the pointer and zeroes on request", async ({ page }) => {
  await open(page, "mousetest", t["nav.mousetest"]);
  const now = page.getByTestId("mouse-now");
  await expect(page.getByText(t["tools.mouse.pad.title"])).toBeVisible();
  await expect(now).toContainText("—");
  await circles(page, "mouse-pad");
  await expect(now).not.toContainText("—");
  const hz = Number((await now.innerText()).replace(/\D+/g, ""));
  expect(hz).toBeGreaterThan(0);
  // Samples, average, maximum and the source badge are all there.
  await expect(page.getByText(t["tools.mouse.samples"])).toBeVisible();
  await expect(
    page.getByText(t["tools.mouse.source.raw"]).or(page.getByText(t["tools.mouse.source.grouped"])),
  ).toBeVisible();
  await page.getByRole("button", { name: t["tools.reset"] }).click();
  await expect(now).toContainText("—");
});

test("the buttons test lights each button, counts clicks and flags a false double click", async ({
  page,
}) => {
  await open(page, "mousetest", t["nav.mousetest"]);
  await page.getByRole("tab", { name: t["tools.mouse.buttons.title"] }).click();
  const pad = page.getByTestId("click-pad");
  await expect(pad).toBeVisible();
  const status = page.getByTestId("bounce-status");
  await expect(status).toContainText(t["tools.mouse.bounce.idle"]);

  const box = await pad.boundingBox();
  if (!box) throw new Error("no pad");
  // The empty corner of the pad: away from the text, still inside it.
  await page.mouse.move(box.x + 24, box.y + 24);
  for (const button of ["left", "right", "middle"] as const) {
    await page.mouse.down({ button });
    await page.mouse.up({ button });
    await page.waitForTimeout(120);
  }
  await expect(page.getByTestId("count-left")).toHaveText("1");
  await expect(page.getByTestId("count-right")).toHaveText("1");
  await expect(page.getByTestId("count-middle")).toHaveText("1");
  await expect(page.getByTestId("count-back")).toHaveText("0");

  // Two slow clicks are a person; no warning.
  await page.mouse.down();
  await page.mouse.up();
  await page.waitForTimeout(200);
  await page.mouse.down();
  await page.mouse.up();
  await expect(page.getByTestId("count-left")).toHaveText("3");
  await expect(status).not.toContainText(t["tools.mouse.bounce.title"]);

  // Two presses in the same instant are a switch that bounced.
  await page.getByTestId("click-pad").evaluate((el) => {
    const press = () => el.dispatchEvent(new MouseEvent("mousedown", { button: 0, bubbles: true }));
    press();
    press();
  });
  await expect(page.getByTestId("count-left")).toHaveText("5");
  await expect(status).toContainText(t["tools.mouse.bounce.title"]);
  await expect(status).toContainText(t["tools.mouse.btn.left"]);

  // The wheel counts both ways.
  await page.mouse.wheel(0, 100);
  await page.mouse.wheel(0, 100);
  await page.mouse.wheel(0, -100);
  await expect(page.getByTestId("wheel-down")).toHaveText("2");
  await expect(page.getByTestId("wheel-up")).toHaveText("1");

  await page.getByRole("button", { name: t["tools.reset"] }).click();
  await expect(page.getByTestId("count-left")).toHaveText("0");
  await expect(status).toContainText(t["tools.mouse.bounce.idle"]);
});

test.describe("reduced motion", () => {
  test.use({ colorScheme: "dark", reducedMotion: "reduce" });

  test("the decorative rings stand still and everything is visible", async ({ page }) => {
    await open(page, "mousetest", t["nav.mousetest"]);
    const rings = page.getByTestId("mouse-pad").locator("div").first();
    expect(await rings.evaluate((e) => getComputedStyle(e).animationName)).toBe("none");
    await expect(page.getByText(t["tools.mouse.pad.title"])).toBeVisible();
  });
});

test.describe("normal motion", () => {
  test.use({ reducedMotion: "no-preference" });

  test("the rings turn slowly", async ({ page }) => {
    await open(page, "mousetest", t["nav.mousetest"]);
    const rings = page.getByTestId("mouse-pad").locator("div").first();
    expect(await rings.evaluate((e) => getComputedStyle(e).animationName)).toBe("spin");
  });
});
