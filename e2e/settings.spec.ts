import { expect, open, t, test } from "./fixtures";

test("settings render", async ({ page }) => {
  await open(page, "settings", t["settings.title"]);
  await expect(page.getByText(t["settings.autostart"], { exact: true })).toBeVisible();
});
