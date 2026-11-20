import { expect, test } from "bun:test";
import type { AppEntry, Leftover, StartupItem } from "../src/lib/backend";
import { groupLeftovers, staleBigApps, startupByExe } from "../src/lib/ram-apps";

const MB = 1024 ** 2;
const DAY = 86_400;
const NOW = 1_800_000_000;

const app = (id: string, size: number | null, protectedBy: AppEntry["protected"] = null): AppEntry => ({
  id,
  hive: "user",
  key: id,
  name: id,
  publisher: "",
  version: "",
  install_location: null,
  exe: null,
  icon_source: null,
  size_bytes: size,
  install_date: null,
  uninstall: "",
  msi: false,
  needs_admin: false,
  protected: protectedBy,
});

test("rarely used and big: 90+ days without use and 500 MB+, biggest first", () => {
  const apps = [
    app("old-big", 2000 * MB),
    app("old-bigger", 5000 * MB),
    app("old-small", 100 * MB),
    app("recent-big", 3000 * MB),
    app("never-seen", 9000 * MB),
    app("no-size", null),
    app("protected", 9000 * MB, "runtime"),
  ];
  const usage = {
    "old-big": NOW - 120 * DAY,
    "old-bigger": NOW - 400 * DAY,
    "old-small": NOW - 400 * DAY,
    "recent-big": NOW - 10 * DAY,
    "never-seen": null,
    "no-size": NOW - 400 * DAY,
    protected: NOW - 400 * DAY,
  };
  expect(staleBigApps(apps, usage, NOW).map((a) => a.id)).toEqual(["old-bigger", "old-big"]);
});

test("no launch data: nothing is suggested (never a guess)", () => {
  expect(staleBigApps([app("a", 9000 * MB)], {}, NOW)).toEqual([]);
  expect(staleBigApps([app("a", 9000 * MB)], { a: null }, NOW)).toEqual([]);
  expect(staleBigApps([app("a", 9000 * MB)], { a: 0 }, NOW)).toEqual([]);
});

const left = (id: number, path: string, bytes: number, icon?: string): Leftover => ({
  id,
  kind: "folder",
  path,
  bytes,
  confidence: "high",
  reason: "unowned",
  needs_admin: false,
  precious: false,
  icon,
});

test("one app's leftovers are one group with the app's name and total", () => {
  const g = groupLeftovers(
    [left(0, "C:\\U\\AppData\\Roaming\\Foo", 10), left(1, "HKCU\\Software\\Foo", 0)],
    false,
    "Foo App",
  );
  expect(g).toHaveLength(1);
  expect(g[0]).toMatchObject({ key: "app", name: "Foo App", bytes: 10, icon: null });
});

test("the sweep groups folders by name, first icon found, biggest group first", () => {
  const g = groupLeftovers(
    [
      left(0, "C:\\U\\AppData\\Roaming\\Foo", 10),
      left(1, "C:\\ProgramData\\FOO\\", 30, "C:\\ProgramData\\FOO\\foo.exe"),
      left(2, "D:\\Games\\Bar", 100),
      left(3, "C:\\U\\AppData\\Local\\Foo", 5, "C:\\U\\AppData\\Local\\Foo\\other.exe"),
    ],
    true,
    "sweep",
  );
  expect(g.map((x) => [x.name, x.bytes, x.items.length])).toEqual([
    ["Bar", 100, 1],
    ["Foo", 45, 3],
  ]);
  expect(g[1]?.icon).toBe("C:\\ProgramData\\FOO\\foo.exe");
});

test("startup items by the program they start, case-insensitive", () => {
  const item = (name: string, exe: string | null): StartupItem => ({
    name,
    command: "",
    location: "user_run",
    enabled: true,
    needs_admin: false,
    protected: false,
    exe,
  });
  const m = startupByExe([item("Discord", "C:\\Apps\\Discord.exe"), item("Broken", null)]);
  expect(m.get("c:\\apps\\discord.exe")?.name).toBe("Discord");
  expect(m.size).toBe(1);
  expect(startupByExe(null).size).toBe(0);
});
