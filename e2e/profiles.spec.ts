import { expect, open, t, test } from "./fixtures";

test("profiles page lists the ready-made profiles and an empty list of yours", async ({ page }) => {
  await open(page, "profiles", t["nav.profiles"]);
  await expect(page.getByRole("heading", { name: t["profiles.builtin.title"] })).toBeVisible();
  for (const name of [
    t["boost.preset.default"],
    t["boost.preset.high"],
    t["profiles.builtin.no_ai"],
    t["profiles.builtin.windows"],
  ]) {
    await expect(page.getByRole("heading", { level: 3, name, exact: true })).toBeVisible();
  }
  await expect(page.getByText(t["profiles.custom.empty.title"], { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: t["profiles.create"] })).toBeVisible();
});

test("creating a profile asks for a name and finds tweaks by search", async ({ page }) => {
  await open(page, "profiles", t["nav.profiles"]);
  await page.getByRole("button", { name: t["profiles.create"] }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading", { name: t["profiles.editor.create"] })).toBeVisible();
  const save = dialog.getByRole("button", { name: t["profiles.editor.save"] });
  await expect(save).toBeDisabled();
  await dialog.getByLabel(t["profiles.editor.name"]).fill("Jogos");
  await expect(save).toBeEnabled();
  // The search matches the translated name, ignoring accents and case.
  await dialog.getByLabel(t["profiles.editor.search"]).fill("MOUSE sem");
  await expect(dialog.getByText(t["boost.tweak.mouse"], { exact: true })).toBeVisible();
  await expect(dialog.getByText(t["boost.tweak.keyboard"], { exact: true })).toHaveCount(0);
  await dialog.getByLabel(t["profiles.editor.search"]).fill("zzzz");
  await expect(dialog.getByText(t["profiles.editor.none_found"])).toBeVisible();
  // Without the app there is nowhere to save: the error is shown, nothing crashes.
  await dialog.getByLabel(t["profiles.editor.search"]).fill("");
  await save.click();
  await expect(dialog.getByText(t["profiles.error.app_only"])).toBeVisible();
  await dialog.getByRole("button", { name: t["common.cancel"] }).click();
  await expect(dialog).toBeHidden();
});

test("the apply dialog names the profile and can be cancelled", async ({ page }) => {
  await open(page, "profiles", t["nav.profiles"]);
  const card = page.locator('[data-profile="high"]');
  await card.getByRole("button", { name: t["profiles.apply"] }).click();
  const dialog = page.getByRole("dialog");
  await expect(
    dialog.getByRole("heading", { name: t["profiles.apply.title"].replace("{name}", t["boost.preset.high"]) }),
  ).toBeVisible();
  await expect(dialog.getByRole("button", { name: t["profiles.apply.go"] })).toBeDisabled();
  await dialog.getByRole("button", { name: t["common.cancel"] }).click();
  await expect(dialog).toBeHidden();
});

test("the import dialog takes pasted text", async ({ page }) => {
  await open(page, "profiles", t["nav.profiles"]);
  await page.getByRole("button", { name: t["profiles.import"] }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading", { name: t["profiles.import.title"] })).toBeVisible();
  await expect(dialog.getByText(t["profiles.import.files.empty"])).toBeVisible();
  const paste = dialog.getByLabel(t["profiles.import.paste"]);
  const go = dialog.getByRole("button", { name: t["profiles.import.go"] });
  await expect(go).toBeDisabled();
  await paste.fill('{"name":"X","tweaks":["mouse"]}');
  await expect(go).toBeEnabled();
});

test("the restore tab shows its empty states", async ({ page }) => {
  await open(page, "profiles", t["nav.profiles"], "restore");
  await expect(page.getByRole("tab", { name: t["profiles.tab.restore"] })).toHaveAttribute(
    "data-state",
    "active",
  );
  await expect(page.getByText(t["profiles.restore.empty.title"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["profiles.others.empty"])).toBeVisible();
  await expect(page.getByText(t["profiles.history.empty"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["profiles.restore.undo_all"] })).toBeDisabled();
});
