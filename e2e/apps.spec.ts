import type { Page } from "@playwright/test";
import { expect, open, t, test } from "./fixtures";

test("uninstaller renders both tabs without a backend", async ({ page }) => {
  await open(page, "apps", t["apps.title"], "installed");
  await expect(page.getByText(t["apps.pick.title"])).toBeVisible();
  await page.getByRole("tab", { name: t["apps.tab.orphans"] }).click();
  await expect(page.getByRole("button", { name: t["apps.orphans.scan"] })).toBeDisabled();
});

test("updates tab renders both sections without a backend", async ({ page }) => {
  await open(page, "apps", t["apps.title"], "updates");
  await expect(page.getByRole("tab", { name: t["updates.tab"] })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByRole("button", { name: t["updates.apps.search"] })).toBeDisabled();
  await expect(page.getByRole("button", { name: t["updates.wu.check"] })).toBeDisabled();
  await expect(page.getByText(t["updates.wu.admin.title"])).toBeVisible();
});

test("install is the first tab and renders its empty state without a backend", async ({
  page,
}) => {
  await open(page, "apps", t["apps.title"]);
  await expect(page.getByRole("tab").first()).toHaveText(t["install.tab"]);
  await expect(page.getByRole("tab", { name: t["install.tab"] })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByText(t["install.unavailable"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["install.essentials"] })).toBeDisabled();
  await expect(page.getByRole("button", { name: t["install.refresh"] })).toBeDisabled();
  // The category chips are static; the "selected" one waits for a selection.
  await expect(page.getByRole("button", { name: new RegExp(t["install.cat.games"]) })).toBeVisible();
  await expect(page.getByRole("button", { name: /^Selecionados \(0\)/ })).toBeDisabled();
  await page.getByRole("tab", { name: t["apps.tab.installed"] }).click();
  await expect(page.getByText(t["apps.pick.title"])).toBeVisible();
});

/* ---- the tab against a faked backend (the real one only exists inside Tauri) ---- */

type Mock = { log: [string, Record<string, unknown> | undefined][] };

/** A tiny Tauri: commands answer from a script, `install:progress` events come from `install_run`. */
async function fakeBackend(page: Page) {
  await page.addInitScript(() => {
    const catalog = [
      { id: "Google.Chrome", name: "Google Chrome", category: "browsers", essential: true },
      { id: "Brave.Brave", name: "Brave", category: "browsers", essential: false },
      { id: "Opera.Opera", name: "Opera", category: "browsers", essential: false },
      { id: "Discord.Discord", name: "Discord", category: "communication", essential: true },
      { id: "Valve.Steam", name: "Steam", category: "games", essential: true },
      { id: "7zip.7zip", name: "7-Zip", category: "utilities", essential: true },
    ];
    const installed: Record<string, { version: string; update: string | null }> = {
      "Google.Chrome": { version: "155.0", update: null },
      "Valve.Steam": { version: "2.10", update: "2.11" },
    };
    const callbacks = new Map<number, (e: unknown) => void>();
    const listeners = new Map<string, number[]>();
    let next = 1;
    const w = window as unknown as Record<string, unknown>;
    const mock: Mock = { log: [] };
    w.__mock = mock;
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
        mock.log.push([cmd, args]);
        const emit = (event: string, payload: unknown) => {
          for (const h of listeners.get(event) ?? []) callbacks.get(h)?.({ event, id: 0, payload });
        };
        switch (cmd) {
          case "plugin:event|listen": {
            const a = args as { event: string; handler: number };
            listeners.set(a.event, [...(listeners.get(a.event) ?? []), a.handler]);
            return a.handler;
          }
          case "app_info":
            return {
              version: "0.9.0",
              core_version: "0.9.0",
              os: "Windows 11",
              elevated: false,
              user: "Test",
              start_route: "apps",
              start_tab: "install",
              autorun: false,
              scan_path: null,
              exe: "",
            };
          case "install_catalog":
            return catalog;
          case "install_status":
            return {
              winget: true,
              states: catalog.map((e) => ({
                id: e.id,
                installed: e.id in installed,
                version: installed[e.id]?.version ?? null,
                update: installed[e.id]?.update ?? null,
                via: e.id in installed ? "winget" : null,
                app: e.id === "Google.Chrome" ? "m32:Chrome" : null,
              })),
            };
          case "apps_list":
            return [
              {
                id: "m32:Chrome",
                hive: "machine32",
                key: "Chrome",
                name: "Google Chrome",
                publisher: "Google LLC",
                version: "155.0",
                install_location: null,
                exe: null,
                icon_source: null,
                size_bytes: null,
                install_date: null,
                uninstall: "",
                msi: false,
                needs_admin: true,
                protected: null,
              },
            ];
          case "install_confirm":
            return { token: 7, ids: (args as { ids: string[] }).ids };
          case "install_run": {
            const ids = (args as { ids: string[] }).ids;
            const items = [];
            for (const [index, id] of ids.entries()) {
              const name = catalog.find((e) => e.id === id)?.name ?? id;
              const base = { index, total: ids.length, id, name, code: 0 };
              emit("install:progress", { ...base, status: "running", reason: "" });
              // The second package fails: its app was open.
              const done =
                index === 1
                  ? { status: "failed", reason: "in_use", code: 0x8a150101 }
                  : { status: "ok", reason: "ok", code: 0 };
              if (done.status === "ok") installed[id] = { version: "1.0", update: null };
              emit("install:progress", { ...base, ...done });
              items.push({ id, name, ...done });
            }
            return { at: 1_700_000_000, kind: "install", items, reboot: false, cancelled: false };
          }
          default:
            return null;
        }
      },
    };
  });
}

test("install: pick in order, confirm the exact list, see the result, keep what failed", async ({
  page,
}) => {
  await fakeBackend(page);
  await open(page, "apps", t["apps.title"]);

  await expect(page.getByRole("checkbox", { name: "Brave", exact: true })).toBeVisible();
  // What is installed is a plain tile with a status line, not a checkbox.
  await expect(page.getByRole("checkbox", { name: "Google Chrome" })).toHaveCount(0);
  await expect(page.getByText(t["install.tile.installed"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["install.tile.update"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["install.updates.banner.one"])).toBeVisible();

  // The order of the ticks is the order of the install.
  await page.getByRole("checkbox", { name: "Opera", exact: true }).check({ force: true });
  await page.getByRole("checkbox", { name: "Brave", exact: true }).check({ force: true });
  const dock = page.getByRole("region", { name: t["install.title"] });
  await expect(dock).toContainText("2");
  await expect(page.getByRole("button", { name: /^Selecionados \(2\)/ })).toBeEnabled();

  // The essentials are what is missing from the preset (Chrome and Steam are installed).
  await page.getByRole("button", { name: t["install.essentials"] }).click();
  await expect(dock).toContainText("4");
  await page.getByRole("checkbox", { name: "7-Zip" }).uncheck({ force: true });
  await page.getByRole("checkbox", { name: "Discord", exact: true }).uncheck({ force: true });
  await expect(dock).toContainText("2");

  await dock.getByRole("button", { name: t["install.dock.go"] }).click();
  const dialog = page.getByRole("dialog");
  await expect(dialog.getByRole("heading", { name: "Instalar 2 apps?" })).toBeVisible();
  await expect(dialog.getByText(t["install.confirm.uac"])).toBeVisible();
  await expect(dialog.getByRole("listitem")).toHaveText([/Opera/, /Brave/]);
  await dialog.getByRole("button", { name: t["install.confirm.go"] }).click();

  // The result: the second package failed (app open), the first is done.
  await expect(page.getByText(t["updates.reason.in_use"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["install.run.close"] })).toBeVisible();
  // What failed stays ticked for another try; what installed is no longer offered.
  await expect(page.getByRole("button", { name: /^Selecionados \(1\)/ })).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Brave", exact: true })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: "Opera", exact: true })).toHaveCount(0);

  // The core was asked for exactly the ticked ids, in that order, with the token it issued.
  const log = await page.evaluate(() => (window as unknown as { __mock: Mock }).__mock.log);
  expect(log.filter(([c]) => c === "install_confirm")).toEqual([
    ["install_confirm", { ids: ["Opera.Opera", "Brave.Brave"] }],
  ]);
  expect(log.filter(([c]) => c === "install_run")).toEqual([
    ["install_run", { token: 7, ids: ["Opera.Opera", "Brave.Brave"] }],
  ]);

  await page.getByRole("button", { name: t["install.run.close"] }).click();
  await expect(page.getByRole("button", { name: t["install.run.close"] })).toHaveCount(0);
});

test.describe("install tab with reduced motion", () => {
  test.use({ reducedMotion: "reduce" });

  test("everything is there at once and nothing animates", async ({ page }) => {
    await fakeBackend(page);
    await open(page, "apps", t["apps.title"]);
    await page.getByRole("checkbox", { name: "Brave", exact: true }).check({ force: true });
    await expect(page.getByRole("region", { name: t["install.title"] })).toBeVisible();
    const rising = await page
      .locator(".animate-rise")
      .evaluateAll((els) =>
        els.map((e) => ({ name: getComputedStyle(e).animationName, opacity: getComputedStyle(e).opacity })),
      );
    expect(rising.length).toBeGreaterThan(3);
    for (const s of rising) expect(s).toEqual({ name: "none", opacity: "1" });
  });
});

test("install: an installed app goes to the native uninstaller; updates have their own tab", async ({
  page,
}) => {
  await fakeBackend(page);
  await open(page, "apps", t["apps.title"]);

  // Uninstalling is not winget's job: the tile hands the app to the Installed tab, selected.
  await page.getByRole("button", { name: `${t["install.uninstall"]}: Google Chrome` }).click();
  await expect(page.getByRole("tab", { name: t["apps.tab.installed"] })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByRole("heading", { level: 3, name: "Google Chrome" })).toBeVisible();
  await expect(
    page.getByRole("button", { name: t["apps.uninstall"], exact: true }),
  ).toBeEnabled();

  // "Update all" lives in the Updates tab, one click from the install tab's banner.
  await page.getByRole("tab", { name: t["install.tab"] }).click();
  await page.getByRole("button", { name: t["install.updates.all"] }).click();
  await expect(page.getByRole("tab", { name: t["updates.tab"] })).toHaveAttribute(
    "aria-selected",
    "true",
  );
});
