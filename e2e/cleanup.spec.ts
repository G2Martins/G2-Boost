import { expect, open, t, test } from "./fixtures";

test("cleanup lists the catalog categories", async ({ page }) => {
  await open(page, "cleanup", t["cleanup.title"]);
  await expect(page.getByText(t["cleanup.cat.temp_user"], { exact: true })).toBeVisible();
});
