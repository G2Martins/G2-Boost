import { expect, open, t, test } from "./fixtures";

test("network monitor and scanner render", async ({ page }) => {
  await open(page, "network", t["network.title"]);
  await expect(page.getByText(t["network.monitor.title"]).first()).toBeVisible();
  await page.getByRole("tab", { name: t["network.tab.scanner"] }).click();
  await expect(page.getByText(t["network.download"]).first()).toBeVisible();
});
