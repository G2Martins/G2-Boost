import { expect, open, t, test } from "./fixtures";

test("disk analyzer shows the empty state and its tabs", async ({ page }) => {
  await open(page, "disk", t["disk.title"]);
  await expect(page.getByText(t["disk.empty.title"]).first()).toBeVisible();
  await page.getByRole("tab", { name: t["disk.tab.largest"] }).click();
  await expect(page.getByText(t["disk.empty.title"]).first()).toBeVisible();
});
