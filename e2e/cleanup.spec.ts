import { expect, open, t, test } from "./fixtures";

test("cleanup opens on the smart view, honest outside the app", async ({ page }) => {
  await open(page, "cleanup", t["cleanup.title"]);
  await expect(page.getByText(t["clean.idle.title"])).toBeVisible();
  await expect(page.getByText(t["clean.app_only"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["clean.analyze"] })).toBeDisabled();
  // The drive vessel is there with no invented numbers.
  const vessel = page.getByRole("img", { name: /C:/ });
  await expect(vessel).toBeVisible();
  await expect(vessel).toHaveAttribute("aria-label", /—/);
  await expect(page.getByText(t["cleanup.history.empty"])).toBeVisible();
});

test("detailed mode keeps every category one click away", async ({ page }) => {
  await open(page, "cleanup", t["cleanup.title"]);
  await page.getByRole("button", { name: t["clean.mode.detailed"] }).first().click();
  await expect(page.getByText(t["cleanup.cat.temp_user"], { exact: true })).toBeVisible();
  await page.getByRole("tab", { name: t["cleanup.tab.monthly"] }).click();
  await expect(page.getByText(t["cleanup.cat.driver_installers"], { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: t["cleanup.preview"] })).toBeDisabled();
  await page.getByRole("button", { name: t["clean.mode.smart"] }).click();
  await expect(page.getByText(t["clean.idle.title"])).toBeVisible();
});

test("cleanup system files tab lists the Windows handlers, risky ones opt-in", async ({ page }) => {
  await open(page, "cleanup", t["cleanup.title"], "system");
  await expect(page.getByText(t["volcache.h.sys_setup_logs"], { exact: true })).toBeVisible();
  const risky = page.getByRole("switch", { name: t["volcache.h.sys_previous_installations"] });
  await expect(risky).toHaveAttribute("data-state", "unchecked");
  await expect(page.getByText(t["volcache.h.sys_update_cleanup.warn"])).toBeVisible();
});

test.describe("reduced motion", () => {
  test.use({ reducedMotion: "reduce" });

  test("the vessel and the cards stand still", async ({ page }) => {
    await open(page, "cleanup", t["cleanup.title"]);
    const segments = page.getByRole("img", { name: /C:/ }).locator("span");
    await expect(segments.first()).toBeAttached();
    const durations = await segments.evaluateAll((els) =>
      els.map((e) => getComputedStyle(e).transitionDuration),
    );
    for (const d of durations) expect(d).toBe("0s");
    const rising = await page
      .locator(".animate-rise")
      .evaluateAll((els) => els.map((e) => getComputedStyle(e).animationName));
    for (const a of rising) expect(a).toBe("none");
  });
});
