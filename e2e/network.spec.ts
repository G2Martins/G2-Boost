import { expect, open, t, test } from "./fixtures";

test("network opens on the slow-internet diagnosis", async ({ page }) => {
  await open(page, "network", t["network.title"]);
  await expect(page.getByText(t["netdiag.title"]).first()).toBeVisible();
  for (const step of ["speed", "pc", "wifi", "devices", "nearby", "fixes", "apply"] as const) {
    await expect(page.getByText(t[`netdiag.step.${step}`]).first()).toBeVisible();
  }
  // Browser mode: the diagnosis needs the core.
  await expect(page.getByRole("button", { name: t["netdiag.start"] })).toBeDisabled();
});

test("network monitor and scanner render", async ({ page }) => {
  await open(page, "network", t["network.title"]);
  await page.getByRole("tab", { name: t["network.tab.monitor"] }).click();
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
