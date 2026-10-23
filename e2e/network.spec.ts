import { expect, open, t, test } from "./fixtures";

test("network monitor and scanner render", async ({ page }) => {
  await open(page, "network", t["network.title"]);
  await expect(page.getByText(t["network.monitor.title"]).first()).toBeVisible();
  await page.getByRole("tab", { name: t["network.tab.scanner"] }).click();
  await expect(page.getByText(t["network.download"]).first()).toBeVisible();
});

test("network tools offer the connection repair", async ({ page }) => {
  await open(page, "network", t["network.title"], "tools");
  await expect(page.getByText(t["maint2.net.title"]).first()).toBeVisible();
  await expect(page.getByText(t["maint2.net.dns.title"])).toBeVisible();
  await expect(page.getByText(t["maint2.net.renew.none"])).toBeVisible();
  // Browser mode: nothing can run without the core.
  await expect(page.getByRole("button", { name: t["maint2.net.dns.button"] })).toBeDisabled();
  await expect(page.getByRole("button", { name: t["maint2.net.reset.button"] })).toBeDisabled();
});
