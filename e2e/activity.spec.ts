import { expect, open, t, test } from "./fixtures";
import { emit, fakeTauri, listening } from "./tauri-bridge";

const line = (id: number, read = false) => ({
  id,
  at: Math.floor(Date.now() / 1000),
  kind: "routine",
  key: "activity.routine.cleanup",
  args: { bytes: "1572864" },
  tone: "ok",
  route: "cleanup",
  read,
});
const lineText = t["activity.routine.cleanup"].replace("{bytes}", "1,5 MB");

test("activity page: both tabs say honestly that the history lives in the app", async ({ page }) => {
  await open(page, "activity", t["activity.title"]);
  await expect(page.getByRole("tab", { name: t["activity.tab.notifications"] })).toBeVisible();
  await expect(page.getByText(t["activity.notifications.empty.title"])).toBeVisible();
  await page.getByRole("tab", { name: t["activity.tab.runs"] }).click();
  await expect(page.getByText(t["activity.runs.empty.title"])).toBeVisible();
  await expect(page.getByText(t["common.app_only"])).toBeVisible();
});

test("bell: empty popover, then 'Ver tudo' opens the page", async ({ page }) => {
  await open(page, "dashboard", t["dash.title"]);
  const bell = page.getByRole("button", { name: t["activity.bell.label"].replace("{count}", "0") });
  await bell.click();
  const pop = page.getByRole("dialog", { name: t["activity.bell"] });
  await expect(pop.getByText(t["activity.bell.empty"])).toBeVisible();
  await pop.getByRole("button", { name: t["activity.bell.all"] }).click();
  await expect(pop).toBeHidden();
  await expect(page.getByRole("heading", { level: 1, name: t["activity.title"] })).toBeVisible();
});

test("a new line counts on the bell, toasts away from its page and opens it", async ({ page }) => {
  await fakeTauri(page, { route: "dashboard" });
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: t["dash.title"] })).toBeVisible();
  await listening(page, "activity:new");
  await emit(page, "activity:new", line(1));

  await expect(page.locator(".bell-count")).toHaveText("1");
  const toast = page.locator(".toast").filter({ hasText: lineText });
  await expect(toast).toBeVisible();

  await page.getByRole("button", { name: t["activity.bell.label"].replace("{count}", "1") }).click();
  const pop = page.getByRole("dialog", { name: t["activity.bell"] });
  await expect(pop.getByText(lineText)).toBeVisible();
  await pop.getByText(lineText).click();
  await expect(page.getByRole("heading", { level: 1, name: t["cleanup.title"] })).toBeVisible();
  // seen once the popover closed
  await expect(page.locator(".bell-count")).toHaveCount(0);
});

test("a job on another page shows the pill and the sidebar dot until it ends", async ({ page }) => {
  await fakeTauri(page, { route: "dashboard" });
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: t["dash.title"] })).toBeVisible();
  await listening(page, "disk:progress");
  const tick = { files: 10, dirs: 2, bytes: 512, errors: 0, current: "", expected: 1024, top: [] };
  await emit(page, "disk:progress", tick);

  const pill = page.getByRole("button", { name: t["jobs.open"].replace("{label}", t["jobs.disk"]) });
  await expect(pill).toBeVisible();
  await expect(pill).toContainText("50%");
  await expect(page.getByRole("button", { name: t["nav.disk"], exact: true }).locator(".nav-busy")).toHaveCount(1);

  await emit(page, "disk:done", { root: "C:\\" });
  await expect(pill).toContainText(t["jobs.done"]);
  await expect(page.getByRole("button", { name: t["nav.disk"], exact: true }).locator(".nav-busy")).toHaveCount(0);
  await expect(pill).toBeHidden({ timeout: 5000 });
});

test("on the job's own page there is no pill (the page shows its progress)", async ({ page }) => {
  await fakeTauri(page, { route: "disk" });
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1, name: t["disk.title"] })).toBeVisible();
  await listening(page, "disk:progress");
  await emit(page, "disk:progress", { files: 1, dirs: 1, bytes: 1, errors: 0, current: "", expected: 0, top: [] });
  await expect(page.getByRole("button", { name: t["nav.disk"], exact: true }).locator(".nav-busy")).toHaveCount(1);
  await expect(page.locator(".job-pill")).toHaveCount(0);
});

test.describe("reduced motion", () => {
  test.use({ reducedMotion: "reduce" });

  test("pill, ring, dot and bell count hold still and stay visible", async ({ page }) => {
    await fakeTauri(page, { route: "dashboard" });
    await page.goto("/");
    await expect(page.getByRole("heading", { level: 1, name: t["dash.title"] })).toBeVisible();
    await listening(page, "activity:new");
    await emit(page, "activity:new", line(2));
    await emit(page, "dupes:progress", { candidates: 3, hashed_bytes: 100, errors: 0 });
    const still = async (selector: string) => {
      const el = page.locator(selector).first();
      await expect(el).toBeVisible();
      expect(await el.evaluate((e) => getComputedStyle(e).animationName)).toBe("none");
      expect(await el.evaluate((e) => getComputedStyle(e).opacity)).toBe("1");
    };
    await still(".bell-count");
    await still(".job-pill");
    await still(".job-ring");
    await still(".nav-busy");
  });
});
