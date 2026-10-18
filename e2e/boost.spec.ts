import { expect, open, t, test } from "./fixtures";

test("PC Boost tweaks, power and processes tabs render", async ({ page }) => {
  await open(page, "boost", t["boost.title"]);
  await expect(page.getByText(t["boost.tweak.visual_fx"], { exact: true })).toBeVisible();
  await page.getByRole("tab", { name: t["boost.tab.power"] }).click();
  await expect(page.getByText(t["power.title"], { exact: true })).toBeVisible();
  await page.getByRole("tab", { name: t["boost.tab.procs"] }).click();
  await expect(page.getByText(t["procs.title"], { exact: true })).toBeVisible();
});

test("boost opens straight on a tab from the route", async ({ page }) => {
  await open(page, "boost", t["boost.title"], "power");
  await expect(page.getByRole("tab", { name: t["boost.tab.power"] })).toHaveAttribute("data-state", "active");
});
