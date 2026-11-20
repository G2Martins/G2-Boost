/** Pure helpers for the RAM tab and the leftovers lists (unit-tested in tests/ram-apps.test.ts). */
import type { AppEntry, Leftover, StartupItem } from "@/lib/backend";

const DAY = 86_400;
const MB = 1024 ** 2;

/**
 * Installed apps not opened for `days` and at least `bytes` big, biggest first. Only apps with a
 * real last-use date count: without launch data there is nothing honest to suggest.
 */
export function staleBigApps(
  apps: AppEntry[],
  usage: Record<string, number | null>,
  nowSec: number,
  days = 90,
  bytes = 500 * MB,
): AppEntry[] {
  return apps
    .filter((a) => {
      const used = usage[a.id];
      return (
        !a.protected &&
        (a.size_bytes ?? 0) >= bytes &&
        typeof used === "number" &&
        used > 0 &&
        nowSec - used >= days * DAY
      );
    })
    .sort((a, b) => (b.size_bytes ?? 0) - (a.size_bytes ?? 0));
}

/** Startup items by the program they start (lowercase path): RAM list ↔ startup list. */
export function startupByExe(items: StartupItem[] | null): Map<string, StartupItem> {
  const m = new Map<string, StartupItem>();
  for (const i of items ?? []) if (i.exe) m.set(i.exe.toLowerCase(), i);
  return m;
}

export type LeftoverGroup = {
  key: string;
  name: string;
  /** Program whose icon stands for the group (first one found). */
  icon: string | null;
  bytes: number;
  items: Leftover[];
};

/**
 * One app's leftovers are one group. The every-drive sweep has no owner, so folders are grouped
 * by name: `Foo` in Roaming, Local and ProgramData is one probable app. Biggest group first.
 */
export function groupLeftovers(
  items: Leftover[],
  sweep: boolean,
  appName: string,
): LeftoverGroup[] {
  const groups = new Map<string, LeftoverGroup>();
  for (const x of items) {
    const leaf = x.path.split(/[\\/]/).filter(Boolean).pop() ?? x.path;
    const key = sweep ? leaf.toLowerCase() : "app";
    const g = groups.get(key) ?? {
      key,
      name: sweep ? leaf : appName,
      icon: null,
      bytes: 0,
      items: [],
    };
    g.items.push(x);
    g.bytes += x.bytes;
    g.icon ??= x.icon ?? null;
    groups.set(key, g);
  }
  return [...groups.values()].sort((a, b) => b.bytes - a.bytes);
}
