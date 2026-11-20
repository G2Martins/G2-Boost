import { expect, open, t, test } from "./fixtures";

// Outside the app nothing reads the BIOS: the panel says so instead of showing fake states.
test("anti-cheat panel in Jogos is honest without the backend", async ({ page }) => {
  await open(page, "games", t["games.title"]);
  const panel = page.locator("section", {
    has: page.getByRole("heading", { level: 3, name: t["anticheat.title"] }),
  });
  await expect(panel).toBeVisible();
  await expect(panel.getByText(t["anticheat.app_only"], { exact: true })).toBeVisible();
  // No made-up verdicts and no restart button without the core.
  await expect(panel.getByText(t["anticheat.ready"], { exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: t["bios.reboot"] })).toHaveCount(0);
});

test("Meu PC shows the compact anti-cheat card", async ({ page }) => {
  await open(page, "mypc", t["nav.mypc"]);
  const card = page.locator("section", {
    has: page.getByRole("heading", { level: 3, name: t["anticheat.compact.title"] }),
  });
  await expect(card).toBeVisible();
  await expect(card.getByText(t["anticheat.app_only"], { exact: true })).toBeVisible();
});
