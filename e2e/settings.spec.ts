import { expect, open, t, test } from "./fixtures";
import { calls, fakeTauri } from "./tauri-bridge";

test("settings render", async ({ page }) => {
  await open(page, "settings", t["settings.title"]);
  await expect(page.getByText(t["settings.autostart"], { exact: true })).toBeVisible();
});

test("two quick switches: the second save carries the first", async ({ page }) => {
  const base = {
    schedule: { mode: "off" },
    routine_categories: [],
    game_exes: [],
    close_to_tray: false,
    restore_before_preset: false,
  };
  await fakeTauri(page, {
    route: "settings",
    delays: { settings_set: 500 },
    answers: { settings_get: base, settings_set: { ...base, close_to_tray: true } },
  });
  await page.goto("/");
  const tray = page.getByRole("switch", { name: t["settings.tray"] });
  const restore = page.getByRole("switch", { name: t["settings.restore_before_preset"] });
  await expect(tray).toBeEnabled();
  await tray.click();
  await restore.click(); // before the first save answered
  await expect
    .poll(async () => (await calls(page)).filter((c) => c.cmd === "settings_set").length)
    .toBe(2);
  const sent = (await calls(page))
    .filter((c) => c.cmd === "settings_set")
    .map((c) => c.args.settings as Record<string, unknown>);
  expect(sent[1]).toMatchObject({ close_to_tray: true, restore_before_preset: true });
});
