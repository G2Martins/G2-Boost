// Pure helpers of the hardware monitor page (tests/hw.test.ts): number formatting by unit,
// min/max tracking, where a value sits in its range, the CSV rows, remembered collapsed nodes.
import type { HwCsvRow, HwSnapshot } from "@/lib/backend";
import { formatBytes } from "@/lib/utils";

export type Stat = { min: number; max: number };

/** Fixed decimals per unit so a live column keeps its width (45,0 then 45,3, never 45 then 45,3). */
const DECIMALS: Record<string, number> = {
  "%": 1,
  "°C": 1,
  W: 1,
  V: 3,
  A: 2,
  MHz: 0,
  RPM: 0,
  x: 2,
};

const FORMATS = new Map<string, Intl.NumberFormat>();

/** One formatter per (locale, digits): a board with 700 sensors formats 2000 numbers a second. */
function numberFormat(locale: string, digits: number): Intl.NumberFormat {
  const key = `${locale}|${digits}`;
  let f = FORMATS.get(key);
  if (!f) {
    f = new Intl.NumberFormat(locale, {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    });
    FORMATS.set(key, f);
  }
  return f;
}

/** `12,5%`, `45,3 °C`, `1,5 MB/s`, `12,096 V`. Bytes scale themselves; other units keep what the tool says. */
export function formatSensor(value: number, unit: string, locale: string): string {
  if (!Number.isFinite(value)) return "—";
  if (unit === "B") return formatBytes(value, locale, 1);
  if (unit === "B/s") return `${formatBytes(value, locale, 1)}/s`;
  const abs = Math.abs(value);
  const digits = DECIMALS[unit] ?? (abs >= 100 ? 0 : abs >= 10 ? 1 : 2);
  const n = numberFormat(locale, digits).format(value);
  if (unit === "") return n;
  return unit === "%" ? `${n}%` : `${n} ${unit}`;
}

/** Min/max of every sensor since the page opened; mutates `stats`. */
export function trackStats(stats: Map<string, Stat>, snap: HwSnapshot): void {
  for (const d of snap.devices) {
    for (const g of d.groups) {
      for (const s of g.sensors) {
        const cur = stats.get(s.id);
        if (!cur) stats.set(s.id, { min: s.value, max: s.value });
        else {
          if (s.value < cur.min) cur.min = s.value;
          if (s.value > cur.max) cur.max = s.value;
        }
      }
    }
  }
}

/** Where `value` sits between the lowest and highest seen (0..1); percentages use 0..100. */
export function position(value: number, unit: string, stat: Stat | undefined): number | null {
  const lo = unit === "%" ? 0 : stat?.min;
  const hi = unit === "%" ? 100 : stat?.max;
  if (lo === undefined || hi === undefined || !(hi > lo)) return null;
  return Math.min(1, Math.max(0, (value - lo) / (hi - lo)));
}

export type Labelled = { key: string | null; n?: number | null; label: string };
/** Which dictionary a built-in label lives in: devices `hw.d.*`, groups `hw.g.*`, sensors `hw.s.*`. */
export type Scope = "d" | "g" | "s";

/** The rows of the CSV, flat, with the labels as the page shows them. */
export function csvRows(
  snap: HwSnapshot,
  stats: Map<string, Stat>,
  label: (scope: Scope, x: Labelled) => string,
): HwCsvRow[] {
  const rows: HwCsvRow[] = [];
  for (const d of snap.devices) {
    for (const g of d.groups) {
      for (const s of g.sensors) {
        const st = stats.get(s.id);
        rows.push({
          device: label("d", d),
          group: label("g", g),
          sensor: label("s", s),
          value: Number.isFinite(s.value) ? s.value : null,
          min: st && Number.isFinite(st.min) ? st.min : null,
          max: st && Number.isFinite(st.max) ? st.max : null,
          unit: s.unit,
        });
      }
    }
  }
  return rows;
}

/** Ids of every device (what "collapse all" folds). */
export function deviceIds(snap: HwSnapshot | null): string[] {
  return snap?.devices.map((d) => d.id) ?? [];
}

const COLLAPSED_KEY = "g2boost.hw.collapsed";

/** Remembered between visits; a convenience only, so every storage failure is swallowed. */
export function loadCollapsed(): Set<string> {
  try {
    const raw = JSON.parse(localStorage.getItem(COLLAPSED_KEY) ?? "[]") as unknown;
    if (!Array.isArray(raw)) return new Set();
    return new Set(raw.filter((x): x is string => typeof x === "string").slice(0, 2000));
  } catch {
    return new Set();
  }
}

export function saveCollapsed(s: Set<string>): void {
  try {
    localStorage.setItem(COLLAPSED_KEY, JSON.stringify([...s]));
  } catch {
    /* storage unavailable (private window, blocked site data): the page works without it */
  }
}
