import { expect, open, t, test } from "./fixtures";

const select = (name: string) => t["debloat.select"].replace("{name}", name);

test("debloat page renders the levels, the AI card and the preinstalled apps", async ({ page }) => {
  await open(page, "debloat", t["nav.debloat"]);
  await expect(page.getByText(t["debloat.subtitle"])).toBeVisible();
  for (const level of ["essential", "recommended", "advanced"] as const) {
    await expect(page.getByText(t[`debloat.level.${level}`], { exact: true })).toBeVisible();
  }
  await expect(page.getByRole("heading", { name: t["debloat.ai.title"] })).toBeVisible();
  await expect(page.getByRole("button", { name: t["debloat.ai.apply"] })).toBeDisabled();
  await expect(page.getByText(t["debloat.apps.title"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["debloat.apps.empty"])).toBeVisible();
});

test("select all, clear and see what a tweak changes", async ({ page }) => {
  await open(page, "debloat", t["nav.debloat"]);
  const apply = page.getByRole("button", { name: /^Aplicar selecionados/ });
  await expect(apply).toBeDisabled();
  await page.getByRole("button", { name: t["debloat.select_all"], exact: true }).click();
  await expect(apply).toBeEnabled();
  // Not elevated: only what needs no administrator can be picked.
  await expect(page.getByRole("checkbox", { name: select(t["boost.tweak.tips"]) })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: select(t["boost.tweak.copilot_off"]) })).toBeDisabled();
  await page.getByRole("button", { name: t["debloat.select_none"], exact: true }).click();
  await expect(apply).toBeDisabled();

  await page.getByRole("button", { name: t["tweaks.changes"], exact: true }).first().click();
  const hide = page.getByRole("button", { name: t["tweaks.changes.hide"], exact: true });
  await expect(hide).toHaveAttribute("aria-expanded", "true");
  await expect(page.getByText(t["tweaks.changes.app_only"])).toBeVisible();
  await hide.click();
  await expect(page.getByText(t["tweaks.changes.app_only"])).toHaveCount(0);
});
