// Wording helpers of the Meu PC check-up (tests/checkup.test.ts): the core returns numbers and
// names; these turn them into the sentences the rows show.
import type { CheckupItem } from "@/lib/backend";
import type { I18nKey } from "@/lib/i18n";
import { formatBytes, formatMs } from "@/lib/utils";

type Params = Record<string, string | number>;
export type T = (key: I18nKey, params?: Params) => string;

/** The rows by section, in the order the core measures them. */
export const SECTIONS = [
  { id: "storage", ids: ["disk_free", "ram_use", "disk_health"] },
  { id: "security", ids: ["antivirus", "firewall", "uac", "secure_boot", "tpm"] },
  {
    id: "windows",
    ids: ["activation", "windows_update", "reboot", "shutdowns", "crashes", "startup", "internet"],
  },
  { id: "firmware", ids: ["gpu_driver", "bios_age", "xmp", "rebar"] },
] as const;

/** `1 dia`, `45 dias`, `3 meses`, `1,3 anos`: an age reads better in its own scale. */
export function formatDays(days: number, t: T, locale: string): string {
  const n = Math.max(0, Math.round(days));
  if (n === 1) return t("checkup.day");
  if (n < 60) return t("checkup.days", { n });
  if (n < 730) return t("checkup.months", { n: Math.round(n / 30.4) });
  const years = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(n / 365);
  return t("checkup.years", { n: years });
}

/** A number of a row in the unit the core measured it in; null for yes/no rows. */
function formatUnit(
  unit: CheckupItem["unit"],
  v: number | null,
  t: T,
  locale: string,
): string | null {
  if (v === null || !Number.isFinite(v)) return null;
  switch (unit) {
    case "pct":
      return `${Math.round(v)}%`;
    case "bytes":
      return formatBytes(v, locale, 1);
    case "days":
      return formatDays(v, t, locale);
    case "count":
      return String(Math.round(v));
    case "ms":
      return formatMs(v, locale);
    default:
      return null;
  }
}

/** The measure a row shows on its right; null for yes/no rows (the state says it all). */
export function formatMeasure(item: CheckupItem, t: T, locale: string): string | null {
  return formatUnit(item.unit, item.value, t, locale);
}

/** The line the verdict is judged against ("limite 45 dias"); null when the rule is not a number. */
export function formatLimit(item: CheckupItem, t: T, locale: string): string | null {
  const v = formatUnit(item.unit, item.limit, t, locale);
  return v === null ? null : t("checkup.limit", { limit: v });
}

/**
 * Parameters of a row's sentences: numbers named `*_bytes` / `*_days` are formatted for the
 * locale, the rest is text from the machine; `value` is the measure and `count` its raw number.
 */
export function textParams(item: CheckupItem, t: T, locale: string): Params {
  const out: Params = {};
  for (const [k, raw] of Object.entries(item.params)) {
    const n = Number(raw);
    if (k.endsWith("_bytes") && Number.isFinite(n)) out[k] = formatBytes(n, locale, 1);
    else if (k.endsWith("_days") && Number.isFinite(n)) out[k] = formatDays(n, t, locale);
    else out[k] = raw;
  }
  const measure = formatMeasure(item, t, locale);
  if (measure !== null) out.value = measure;
  if (item.unit === "count" && item.value !== null) out.count = Math.round(item.value);
  return out;
}

/** The sentence under a row's title: what was found, or why nothing could be judged. */
export function rowText(item: CheckupItem, t: T, locale: string): string {
  if (item.state === "unknown")
    return t(`checkup.unknown.${item.reason ?? "unreadable"}` as I18nKey);
  const part = item.state === "ok" ? "ok" : "bad";
  return t(`checkup.${item.id}.${part}` as I18nKey, textParams(item, t, locale));
}

export function rowTitle(item: CheckupItem, t: T, locale: string): string {
  return t(`checkup.${item.id}.title` as I18nKey, textParams(item, t, locale));
}
