import { expect, open, t, test } from "./fixtures";

test("games page shows Game Mode, the library and its options without a backend", async ({ page }) => {
  await open(page, "games", t["games.title"]);
  await expect(page.getByText(t["games.mode.idle"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["games.list.title"]).first()).toBeVisible();
  await expect(page.getByText(t["games.options.power_plan"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["games.list.empty"], { exact: true })).toBeVisible();
});
