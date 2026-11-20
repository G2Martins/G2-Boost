import type { Page } from "@playwright/test";
import { expect, open, t, test } from "./fixtures";

test("RAM tab without a backend: honest empty state, nothing to click", async ({ page }) => {
  await open(page, "boost", t["boost.title"], "ram");
  await expect(page.getByRole("heading", { name: t["ram.title"] })).toBeVisible();
  await expect(page.getByRole("button", { name: t["boost.ram.clean"], exact: true })).toBeDisabled();
  await expect(page.getByText(t["common.app_only"]).first()).toBeVisible();
  await expect(page.getByRole("heading", { name: t["ram.apps.title"] })).toBeVisible();
});

type Log = [string, Record<string, unknown> | undefined][];
const GB = 1024 ** 3;
const MB = 1024 ** 2;

/** A tiny Tauri: RAM, startup and leftovers answer from a script; icons never load (fallbacks). */
async function fakeBackend(page: Page, route: string, tab: string) {
  await page.addInitScript(
    ({ route, tab, GB, MB }) => {
      let purged = false;
      const split = () => ({
        total: 16 * GB,
        in_use: 6 * GB,
        modified: GB / 2,
        standby: purged ? GB / 2 : 9 * GB,
        free: purged ? 9 * GB : GB / 2,
        compressed: null,
        committed: 8 * GB,
        commit_limit: 20 * GB,
        advice: purged ? "fine" : "helps",
      });
      const discord = {
        name: "Discord",
        command: '"C:\\Apps\\Discord\\Update.exe" --processStart Discord.exe',
        location: "user_run",
        enabled: true,
        needs_admin: false,
        protected: false,
        exe: "C:\\Apps\\Discord\\Discord.exe",
      };
      const leftover = (id: number, path: string, bytes: number, icon?: string) => ({
        id,
        kind: "folder",
        path,
        bytes,
        confidence: "high",
        reason: "unowned",
        needs_admin: false,
        precious: false,
        idle_days: 400,
        icon,
      });
      const callbacks = new Map<number, (e: unknown) => void>();
      let next = 1;
      const log: Log = [];
      const w = window as unknown as Record<string, unknown>;
      w.__log = log;
      w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
      w.__TAURI_INTERNALS__ = {
        metadata: {
          currentWindow: { label: "main" },
          currentWebview: { windowLabel: "main", label: "main" },
        },
        transformCallback(cb: (e: unknown) => void) {
          callbacks.set(next, cb);
          return next++;
        },
        unregisterCallback(id: number) {
          callbacks.delete(id);
        },
        async invoke(cmd: string, args?: Record<string, unknown>) {
          log.push([cmd, args]);
          switch (cmd) {
            case "plugin:event|listen":
              return (args as { handler: number }).handler;
            case "app_info":
              return {
                version: "0.9.3",
                core_version: "0.9.3",
                os: "Windows 11",
                elevated: true,
                user: "Test",
                start_route: route,
                start_tab: tab,
                autorun: false,
                scan_path: null,
                exe: "",
                ask_admin: false,
              };
            case "ram_composition":
              return split();
            case "ram_apps":
              return [
                {
                  name: "discord.exe",
                  path: "C:\\Apps\\Discord\\Discord.exe",
                  count: 5,
                  private_bytes: 700 * MB,
                  ws_bytes: 900 * MB,
                  system: false,
                  protected: null,
                  procs: [{ pid: 10, created: "133" }],
                },
                {
                  name: "svchost.exe",
                  path: "C:\\Windows\\System32\\svchost.exe",
                  count: 40,
                  private_bytes: 500 * MB,
                  ws_bytes: 800 * MB,
                  system: true,
                  protected: "system",
                  procs: [],
                },
              ];
            case "startup_list":
              return [discord];
            case "startup_set":
              return { ...discord, enabled: (args as { enabled: boolean }).enabled };
            case "ram_purge":
              purged = true;
              return {
                before_avail: 9.5 * GB,
                after_avail: 9.5 * GB,
                freed_bytes: 8.5 * GB,
                standby_before: 9 * GB,
                standby_after: GB / 2,
                free_before: GB / 2,
                free_after: 9 * GB,
              };
            case "procs_list":
              return [];
            case "apps_leftovers":
              return [
                leftover(0, "C:\\Users\\T\\AppData\\Roaming\\Foo Tool", 300 * MB, "C:\\x\\foo.exe"),
                leftover(1, "D:\\Games\\Bar Game", 2 * GB),
                leftover(2, "C:\\ProgramData\\Foo Tool", 100 * MB),
              ];
            default:
              return null;
          }
        },
      };
    },
    { route, tab, GB, MB },
  );
}

const calls = (page: Page) => page.evaluate(() => (window as unknown as { __log: Log }).__log);

test("RAM: live split, when a purge helps, before/after, who uses it", async ({ page }) => {
  await fakeBackend(page, "boost", "ram");
  await open(page, "boost", t["boost.title"]);
  await expect(page.getByText(t["ram.advice.helps"])).toBeVisible();
  const bar = page.getByRole("img", { name: new RegExp(t["ram.seg.standby"]) });
  await expect(bar).toBeVisible();

  // Real icon or the name's tile: the row is there either way, with what it costs.
  const row = page.getByRole("listitem").filter({ hasText: "discord.exe" });
  await expect(row.getByText("700 MB")).toBeVisible();
  await expect(row.locator("[data-icon-fallback]")).toBeVisible();
  await expect(row.getByText(t["ram.apps.startup"])).toBeVisible();
  // Windows' own processes are shown, never offered to end.
  await expect(page.getByRole("button", { name: "Encerrar svchost.exe" })).toBeDisabled();

  // RAM ↔ startup: the heavy app that starts with Windows is turned off from here.
  await row.getByRole("button", { name: t["ram.apps.startup_off"] }).click();
  await expect(page.getByText("Discord não abre mais com o Windows.")).toBeVisible();
  expect((await calls(page)).filter(([c]) => c === "startup_set")).toEqual([
    ["startup_set", { location: "user_run", name: "Discord", enabled: false }],
  ]);

  // The purge: the cache turns into free memory and the numbers say so.
  await page.getByRole("button", { name: t["boost.ram.clean"], exact: true }).click();
  await expect(page.getByText(t["ram.purge.freed"])).toBeVisible();
  await expect(page.getByText(/Cache em espera: 9 GB → 512 MB/)).toBeVisible();
  await expect(page.getByText(t["ram.advice.fine"])).toBeVisible();

  // RAM → task manager, filtered on that program.
  await page.getByRole("button", { name: "Ver discord.exe no Gerenciador de tarefas" }).click();
  await expect(page.getByRole("tab", { name: t["boost.tab.procs"] })).toHaveAttribute(
    "data-state",
    "active",
  );
  await expect(page.getByRole("searchbox", { name: t["procs.search"] })).toHaveValue("discord.exe");
});

test("startup items show their program and what it uses now", async ({ page }) => {
  await fakeBackend(page, "boost", "startup");
  await open(page, "boost", t["boost.title"]);
  const item = page.getByRole("listitem").filter({ hasText: "Discord" }).first();
  await expect(item.getByText("usa 700 MB agora")).toBeVisible();
  await expect(item.locator("[data-icon-fallback]")).toBeVisible();
  await expect(page.getByRole("heading", { name: t["ram.heavy.title"] })).toBeVisible();
});

test("every-drive sweep: folders grouped by name, icon fallback, select a whole group", async ({
  page,
}) => {
  await fakeBackend(page, "apps", "unowned");
  await open(page, "apps", t["apps.title"]);
  const foo = page.getByRole("checkbox", { name: "Selecionar tudo de Foo Tool" });
  await expect(foo).toBeChecked();
  // The folder's own program was asked for its icon; none came back, so the tile stands in.
  await expect(page.locator("[data-icon-fallback]").first()).toBeVisible();
  expect((await calls(page)).some(([c, a]) => c === "exe_icon" && a?.path === "C:\\x\\foo.exe")).toBe(true);
  await expect(page.getByText("2 itens · 400 MB")).toBeVisible();
  await expect(page.getByText("3 itens · 2,4 GB")).toBeVisible();
  await expect(page.getByText("3 selecionados · 2,4 GB")).toBeVisible();
  await foo.uncheck();
  await expect(page.getByText("1 selecionados · 2 GB")).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "C:\\ProgramData\\Foo Tool" })).not.toBeChecked();
  await page.getByRole("checkbox", { name: "C:\\ProgramData\\Foo Tool" }).check();
  await expect(foo).not.toBeChecked();
  expect(await foo.evaluate((e) => (e as HTMLInputElement).indeterminate)).toBe(true);
});

test.describe("RAM tab with reduced motion", () => {
  test.use({ reducedMotion: "reduce" });

  test("everything is there at once and the bar does not animate", async ({ page }) => {
    await fakeBackend(page, "boost", "ram");
    await open(page, "boost", t["boost.title"]);
    await expect(page.getByText(t["ram.advice.helps"])).toBeVisible();
    const rising = await page
      .locator(".animate-rise")
      .evaluateAll((els) =>
        els.map((e) => ({ name: getComputedStyle(e).animationName, opacity: getComputedStyle(e).opacity })),
      );
    expect(rising.length).toBeGreaterThan(2);
    for (const s of rising) expect(s).toEqual({ name: "none", opacity: "1" });
    const segs = await page
      .locator("[data-seg]")
      .evaluateAll((els) => els.map((e) => getComputedStyle(e).transitionDuration));
    expect(segs.length).toBe(4);
    for (const d of segs) expect(d).toBe("0s");
  });
});
