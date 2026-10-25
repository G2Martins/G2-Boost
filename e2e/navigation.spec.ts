import { expect, open, t, test } from "./fixtures";

const ROUTES = [
  ["nav.disk", "disk.title"],
  ["nav.cleanup", "cleanup.title"],
  ["maintenance.nav", "maintenance.title"],
  ["nav.apps", "apps.title"],
  ["nav.network", "network.title"],
  ["nav.games", "games.title"],
  ["nav.boost", "boost.title"],
  ["nav.console", "console.title"],
  ["nav.ai", "ai.title"],
  ["nav.settings", "settings.title"],
  ["nav.mypc", "nav.mypc"],
  ["nav.hwmonitor", "nav.hwmonitor"],
  ["nav.setup", "nav.setup"],
  ["nav.debloat", "nav.debloat"],
  ["nav.peripherals", "nav.peripherals"],
  ["nav.profiles", "nav.profiles"],
  ["nav.overlay", "nav.overlay"],
  ["nav.mousetest", "nav.mousetest"],
  ["nav.monitortest", "nav.monitortest"],
  ["nav.packetloss", "nav.packetloss"],
  // Before the dashboard: its monitor has a "GPU" metric button with the same name.
  ["gpu.nav", "gpu.title"],
  ["nav.dashboard", "dash.title"],
] as const;

test("the sidebar reaches every page", async ({ page }) => {
  await open(page, "dashboard", t["dash.title"]);
  for (const [nav, title] of ROUTES) {
    await page.getByRole("button", { name: t[nav], exact: true }).click();
    await expect(page.getByRole("heading", { level: 1, name: t[title] })).toBeVisible();
    await expect(page.getByRole("button", { name: t[nav], exact: true })).toHaveAttribute("aria-current", "page");
  }
});
