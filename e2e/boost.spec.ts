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

test("tweaks: search, category filter and what changes", async ({ page }) => {
  await open(page, "boost", t["boost.title"]);
  const search = page.getByRole("searchbox", { name: t["tweaks.search"] });
  await search.fill("copilot");
  await expect(page.getByText(t["boost.tweak.copilot_off"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["boost.tweak.visual_fx"], { exact: true })).toHaveCount(0);
  await search.fill("zzzz");
  await expect(page.getByText(t["tweaks.empty"])).toBeVisible();
  await search.fill("");

  await page.getByRole("button", { name: new RegExp(`^${t["boost.category.ai"]}\\d`) }).click();
  await expect(page.getByText(t["boost.tweak.recall_off"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["boost.tweak.game_mode"], { exact: true })).toHaveCount(0);

  await page.getByText(t["tweaks.changes"], { exact: true }).first().click();
  await expect(page.getByText(t["tweaks.changes.app_only"]).first()).toBeVisible();
  await expect(page.getByRole("button", { name: t["tweaks.apply_recommended"] })).toBeDisabled();
});
