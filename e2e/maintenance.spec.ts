import { expect, open, t, test } from "./fixtures";

test("maintenance shows the repair tools in the recommended order", async ({ page }) => {
  await open(page, "maintenance", t["maintenance.title"]);
  for (const key of [
    "maintenance.dism_scan.title",
    "maintenance.dism_restore.title",
    "maintenance.sfc.title",
    "maintenance.mrt.title",
  ] as const) {
    await expect(page.getByRole("heading", { level: 3, name: t[key] })).toBeVisible();
  }
  await expect(page.getByText(t["maintenance.mrt.not_av"])).toBeVisible();
  // Browser mode = not elevated: the tools cannot start, elevation is offered.
  await expect(page.getByText(t["maintenance.admin.body"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["maintenance.start"] }).first()).toBeDisabled();
});

test("maintenance shows disk check, drive optimization and the boot options restart", async ({
  page,
}) => {
  await open(page, "maintenance", t["maintenance.title"]);
  for (const key of ["maint2.check.title", "maint2.optimize.title"] as const) {
    await expect(page.getByRole("heading", { level: 3, name: t[key] })).toBeVisible();
  }
  // Browser mode: no core, so no volume list.
  await expect(page.getByText(t["maint2.none"]).first()).toBeVisible();
  await expect(page.getByText(t["maint2.boot.title"]).first()).toBeVisible();
  await expect(page.getByText(t["maint2.boot.step.startup"]).first()).toBeVisible();
  await expect(page.getByText(t["maint2.boot.bitlocker"])).toBeVisible();
});
