import { expect, open, t, test } from "./fixtures";

const ROUTES = [
  ["nav.disk", "disk.title"],
  ["nav.cleanup", "cleanup.title"],
  ["nav.apps", "apps.title"],
  ["nav.network", "network.title"],
  ["nav.boost", "boost.title"],
  ["nav.console", "console.title"],
  ["nav.ai", "ai.title"],
  ["nav.settings", "settings.title"],
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
