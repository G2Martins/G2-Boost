import { expect, open, t, test } from "./fixtures";

test("mouse and keyboard page renders its cards without the app", async ({ page }) => {
  await open(page, "peripherals", t["nav.peripherals"]);
  await expect(page.getByRole("heading", { level: 3, name: t["periph.mouse.title"] })).toBeVisible();
  await expect(page.getByRole("heading", { level: 3, name: t["periph.keyboard.title"] })).toBeVisible();
  // The sliders show Windows' defaults, locked: there is no backend to write to.
  const speed = page.getByLabel(t["periph.speed"], { exact: true });
  await expect(speed).toBeDisabled();
  await expect(speed).toHaveValue("10");
  await expect(page.getByText(t["periph.speed.one_to_one"], { exact: true })).toBeVisible();
  await expect(page.getByLabel(t["periph.kb.delay"], { exact: true })).toBeDisabled();
  await expect(page.getByLabel(t["periph.kb.rate"], { exact: true })).toBeDisabled();
});

test("the key repeat box takes typing and the double-click zone reacts", async ({ page }) => {
  await open(page, "peripherals", t["nav.peripherals"]);
  const box = page.getByLabel(t["periph.kb.try"]);
  await box.fill("aaaa");
  await expect(box).toHaveValue("aaaa");
  const zone = page.getByRole("button", { name: t["periph.dblclick.test"] });
  await zone.dblclick();
  await expect(page.getByRole("button", { name: t["periph.dblclick.ok"] })).toBeVisible();
});

test("game shortcuts are listed and locked until the tweaks exist", async ({ page }) => {
  await open(page, "peripherals", t["nav.peripherals"]);
  await expect(page.getByRole("heading", { level: 3, name: t["periph.shortcuts.title"] })).toBeVisible();
  for (const id of ["alt_shift_off", "narrator_shortcut_off", "accessibility_shortcuts_off"] as const) {
    const row = page.locator(`[data-tweak="${id}"]`);
    await expect(row.getByText(t[`periph.shortcut.${id}`], { exact: true })).toBeVisible();
    await expect(row.getByRole("switch")).toBeDisabled();
  }
});

test("the mouse test link goes to the mouse test page", async ({ page }) => {
  await open(page, "peripherals", t["nav.peripherals"]);
  // The sidebar has a button with the same name: the link in question lives in the page.
  await page.getByRole("main").getByRole("button", { name: t["periph.mouse.test"] }).click();
  await expect(page.getByRole("heading", { level: 1, name: t["nav.mousetest"] })).toBeVisible();
});
