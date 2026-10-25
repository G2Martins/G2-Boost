import { expect, open, t, test } from "./fixtures";

const fill = (key: keyof typeof t, game: string) => t[key].replace("{game}", game);

test("games page shows Game Mode, the empty library and its options without a backend", async ({
  page,
}) => {
  await open(page, "games", t["games.title"]);
  await expect(page.getByText(t["games.mode.hint"], { exact: true })).toBeVisible();
  await expect(page.getByRole("heading", { level: 3, name: t["games.lib.title"] })).toBeVisible();
  await expect(page.getByText(t["games.lib.empty.title"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["games.options.power_plan"], { exact: true })).toBeVisible();
  // The Open dialog only exists inside the app: both "add" buttons are off in the browser.
  const add = page.getByRole("button", { name: t["games.lib.add"] });
  await expect(add).toHaveCount(2);
  for (const b of await add.all()) await expect(b).toBeDisabled();
});

test("poster grid: search, launcher filter, keyboard reveal, options and route", async ({ page }) => {
  await open(page, "games", t["games.title"], "demo");
  const posters = page.locator("li.poster");
  await expect(posters).toHaveCount(4);

  // Search ignores accents; the launcher filter narrows; "Todos" brings everything back.
  const search = page.getByRole("searchbox", { name: t["games.lib.search"] });
  await search.fill("tatica");
  await expect(posters).toHaveCount(1);
  await expect(page.getByRole("heading", { level: 3, name: "Arena Tática" })).toBeVisible();
  await search.fill("nada parecido");
  await expect(page.getByText(t["games.lib.no_match"], { exact: true })).toBeVisible();
  await search.fill("");
  // Filter chips are named "<launcher> <count>".
  const chip = (label: string) => page.getByRole("button", { name: new RegExp(`^${label} \\d+$`) });
  await chip(t["games.launcher.epic"]).click();
  await expect(posters).toHaveCount(1);
  await expect(page.getByRole("heading", { level: 3, name: "Corrida Noturna" })).toBeVisible();
  await expect(chip(t["games.launcher.epic"])).toHaveAttribute("aria-pressed", "true");
  await chip(t["games.lib.all"]).click();
  await expect(posters).toHaveCount(4);

  // The actions rest hidden and come forward on keyboard focus.
  const play = page.getByRole("button", {
    name: fill("games.play.aria", "Arena Tática"),
    exact: true,
  });
  const actions = play.locator("..");
  await expect(actions).toHaveCSS("opacity", "0");
  await play.focus();
  await expect(actions).toHaveCSS("opacity", "1");
  // A game known only by its exe name has nothing to start.
  await expect(
    page.getByRole("button", { name: fill("games.play.aria", "jogo"), exact: true }),
  ).toBeDisabled();
  // Launching needs the app: the browser says so instead of failing silently.
  await play.press("Enter");
  await expect(page.getByText(t["common.app_only"], { exact: true })).toBeVisible();

  // Options dialog: Game Mode for the game, its priority and its exe.
  await page.getByRole("button", { name: fill("gametweaks.open.aria", "Reino Perdido") }).click();
  const dialog = page.getByRole("dialog", { name: fill("gametweaks.title", "Reino Perdido") });
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("switch", { name: t["gametweaks.mode"] })).toBeVisible();
  await expect(dialog.getByRole("switch", { name: t["gametweaks.high_priority"] })).toBeVisible();
  await expect(dialog.getByRole("button", { name: t["gametweaks.exe.change"] })).toBeDisabled();
  await dialog.getByRole("button", { name: t["gametweaks.cancel"] }).first().click();
  await expect(dialog).toBeHidden();

  // Rota opens the route panel for that game, and closes it again.
  const route = page.getByRole("button", { name: fill("route.title", "Arena Tática") });
  await route.click();
  await expect(page.getByRole("heading", { name: fill("route.title", "Arena Tática") })).toBeVisible();
  await expect(route).toHaveAttribute("aria-pressed", "true");
  await route.click();
  await expect(page.getByRole("heading", { name: fill("route.title", "Arena Tática") })).toBeHidden();
});
