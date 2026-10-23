import { expect, open, t, test } from "./fixtures";

test("cleanup lists the catalog categories", async ({ page }) => {
  await open(page, "cleanup", t["cleanup.title"]);
  await expect(page.getByText(t["cleanup.cat.temp_user"], { exact: true })).toBeVisible();
});

test("cleanup system files tab lists the Windows handlers, risky ones opt-in", async ({ page }) => {
  await open(page, "cleanup", t["cleanup.title"], "system");
  await expect(page.getByText(t["volcache.h.sys_setup_logs"], { exact: true })).toBeVisible();
  const risky = page.getByRole("switch", { name: t["volcache.h.sys_previous_installations"] });
  await expect(risky).toHaveAttribute("data-state", "unchecked");
  await expect(page.getByText(t["volcache.h.sys_update_cleanup.warn"])).toBeVisible();
});
