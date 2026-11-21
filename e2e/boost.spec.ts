import { expect, open, t, test } from "./fixtures";
import { calls, fakeTauri } from "./tauri-bridge";

test("PC Boost tweaks, power and processes tabs render", async ({ page }) => {
  await open(page, "boost", t["boost.title"]);
  await expect(page.getByText(t["boost.tweak.visual_fx"], { exact: true })).toBeVisible();
  await page.getByRole("tab", { name: t["boost.tab.power"] }).click();
  await expect(page.getByText(t["power.title"], { exact: true })).toBeVisible();
  await page.getByRole("tab", { name: t["boost.tab.procs"] }).click();
  await expect(page.getByText(t["procs.title"], { exact: true })).toBeVisible();
});

test("boost opens straight on a tab from the route", async ({ page }) => {
  await open(page, "boost", t["boost.title"], "power");
  await expect(page.getByRole("tab", { name: t["boost.tab.power"] })).toHaveAttribute("data-state", "active");
});

test("tweaks: search, category filter and what changes", async ({ page }) => {
  await open(page, "boost", t["boost.title"]);
  const search = page.getByRole("searchbox", { name: t["tweaks.search"] });
  await search.fill("copilot");
  await expect(page.getByText(t["boost.tweak.copilot_off"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["boost.tweak.visual_fx"], { exact: true })).toHaveCount(0);
  await search.fill("zzzz");
  await expect(page.getByText(t["tweaks.empty"])).toBeVisible();
  await search.fill("");

  await page.getByRole("button", { name: new RegExp(`^${t["boost.category.ai"]}\\d`) }).click();
  await expect(page.getByText(t["boost.tweak.recall_off"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["boost.tweak.game_mode"], { exact: true })).toHaveCount(0);

  await page.getByText(t["tweaks.changes"], { exact: true }).first().click();
  await expect(page.getByText(t["tweaks.changes.app_only"]).first()).toBeVisible();
  await expect(page.getByRole("button", { name: t["tweaks.apply_recommended"] })).toBeDisabled();
});

test("a start tab is taken once: the next visit opens on the page's default", async ({ page }) => {
  await open(page, "boost", t["boost.title"], "power");
  await expect(page.getByRole("tab", { name: t["boost.tab.power"] })).toHaveAttribute("data-state", "active");
  await page.getByRole("button", { name: t["nav.disk"], exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: t["disk.title"] })).toBeVisible();
  await page.getByRole("button", { name: t["nav.boost"], exact: true }).click();
  await expect(page.getByRole("tab", { name: t["boost.tab.tweaks"] })).toHaveAttribute("data-state", "active");
});

const gameMode = {
  id: "game_mode",
  admin: false,
  beta: false,
  category: "gaming",
  status: "off",
  has_backup: false,
  level: null,
  detail: null,
  services: [],
};

test("a preset is locked from the click on, while the restore point is still being made", async ({
  page,
}) => {
  await fakeTauri(page, {
    route: "boost",
    elevated: true,
    delays: { restore_create: 400 },
    answers: {
      boost_state: [gameMode],
      settings_get: { restore_before_preset: true },
      restore_create: "created",
      boost_apply: { ...gameMode, status: "on" },
    },
  });
  await page.goto("/");
  const high = page.getByRole("button", { name: t["boost.preset.high"], exact: true });
  await expect(high).toBeEnabled();
  await high.dblclick();
  await expect(page.getByText(t["boost.preset.restore_created"])).toBeVisible();
  await expect.poll(async () => (await calls(page)).filter((c) => c.cmd === "boost_apply").length).toBe(1);
  // One batch: one restore point, one apply (the second click found the presets locked).
  const log = await calls(page);
  expect(log.filter((c) => c.cmd === "restore_create")).toHaveLength(1);
  expect(log.filter((c) => c.cmd === "boost_apply")).toHaveLength(1);
});
