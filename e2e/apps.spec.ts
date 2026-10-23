import { expect, open, t, test } from "./fixtures";

test("uninstaller renders both tabs without a backend", async ({ page }) => {
  await open(page, "apps", t["apps.title"]);
  await expect(page.getByText(t["apps.pick.title"])).toBeVisible();
  await page.getByRole("tab", { name: t["apps.tab.orphans"] }).click();
  await expect(page.getByRole("button", { name: t["apps.orphans.scan"] })).toBeDisabled();
});

test("updates tab renders both sections without a backend", async ({ page }) => {
  await open(page, "apps", t["apps.title"], "updates");
  await expect(page.getByRole("tab", { name: t["updates.tab"] })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByRole("button", { name: t["updates.apps.search"] })).toBeDisabled();
  await expect(page.getByRole("button", { name: t["updates.wu.check"] })).toBeDisabled();
  await expect(page.getByText(t["updates.wu.admin.title"])).toBeVisible();
});
