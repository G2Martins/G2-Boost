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
