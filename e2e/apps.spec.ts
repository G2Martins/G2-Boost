import { expect, open, t, test } from "./fixtures";

test("uninstaller renders both tabs without a backend", async ({ page }) => {
  await open(page, "apps", t["apps.title"]);
  await expect(page.getByText(t["apps.pick.title"])).toBeVisible();
  await page.getByRole("tab", { name: t["apps.tab.orphans"] }).click();
  await expect(page.getByRole("button", { name: t["apps.orphans.scan"] })).toBeDisabled();
});
