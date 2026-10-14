import { type ClassValue, clsx } from "clsx";
import { twMerge } from "tailwind-merge";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** 1536 → "1,5 KB" (locale aware). */
export function formatBytes(bytes: number, locale: string, digits = 1): string {
  if (!Number.isFinite(bytes) || bytes < 0) return "—";
  let i = 0;
  let v = bytes;
  while (v >= 1024 && i < UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  const n = new Intl.NumberFormat(locale, {
    maximumFractionDigits: i === 0 ? 0 : digits,
  }).format(v);
  return `${n} ${UNITS[i]}`;
}

export function formatMs(ms: number, locale: string): string {
  if (!Number.isFinite(ms)) return "—";
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: ms < 10 ? 1 : 0 }).format(ms)} ms`;
}

export function formatMbps(mbps: number, locale: string): string {
  if (!Number.isFinite(mbps)) return "—";
  return `${new Intl.NumberFormat(locale, { maximumFractionDigits: mbps < 10 ? 1 : 0 }).format(mbps)} Mbps`;
}
