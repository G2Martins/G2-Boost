// Atividade › Execuções: the module histories the core already keeps (cleanup, repair, drives,
// weekly review, quick fixes, updates, installs, profiles) folded into one timeline. Pure, so
// tests/runs.test.ts covers it; the page renders the keys. Also the bell's line text.
import type { Route } from "@/components/shell/app-shell";
import type { Tone } from "@/components/ui/primitives";
import type {
  ActivityEntry,
  HistoryEntry,
  InstallBatch,
  MaintFixOutcome,
  MaintPlanOutcome,
  RepairOutcome,
  RepairVerdict,
  RunRecord,
  UpdateBatch,
  UpdateItem,
  VolumeOutcome,
  VolumeVerdict,
} from "@/lib/backend";
import type { I18nKey } from "@/lib/i18n";
import { formatBytes } from "@/lib/utils";
import { ptBR } from "@/locales/pt-BR";

export type Params = Record<string, string | number>;
export type T = (key: I18nKey, params?: Params) => string;

export type Run = {
  id: string;
  /** Unix seconds. */
  at: number;
  route: Route;
  tab?: string;
  title: [I18nKey, Params?];
  result: [I18nKey, Params?];
  tone: Tone;
  ms: number | null;
};

export type Histories = {
  cleanup?: RunRecord[] | null;
  repair?: RepairOutcome[] | null;
  volumes?: VolumeOutcome[] | null;
  plans?: MaintPlanOutcome[] | null;
  fixes?: MaintFixOutcome[] | null;
  updates?: UpdateBatch[] | null;
  installs?: InstallBatch[] | null;
  profiles?: HistoryEntry[] | null;
};

/** Newest kept in the timeline. */
export const MAX_RUNS = 300;

const REPAIR_TONE: Record<RepairVerdict, Tone> = {
  healthy: "success",
  clean: "success",
  repaired: "success",
  repairable: "warning",
  threats: "danger",
  non_repairable: "danger",
  failed: "danger",
  cancelled: "neutral",
  unknown: "neutral",
};
const VOLUME_TONE: Record<VolumeVerdict, Tone> = {
  healthy: "success",
  fixed: "success",
  optimized: "success",
  needs_spot_fix: "warning",
  needs_offline: "warning",
  retry: "warning",
  failed: "danger",
  cancelled: "neutral",
};
const TRIGGERS = ["manual", "routine", "game", "app_leftovers", "disk_recycle", "mcp"] as const;
const TRIGGER_ROUTE: Record<string, [Route, string?]> = {
  game: ["boost", "ram"],
  app_leftovers: ["apps", "orphans"],
  disk_recycle: ["disk"],
};

function cleanupRun(r: RunRecord, i: number): Run {
  const freed = r.items.reduce((s, x) => s + x.bytes_freed, 0);
  const errors = r.items.filter((x) => x.error && x.error !== "cancelled").length;
  const stopped = r.items.length > 0 && r.items.every((x) => x.error === "cancelled");
  const system = r.items.length > 0 && r.items.every((x) => x.id.startsWith("sys_"));
  const trigger = (TRIGGERS as readonly string[]).includes(r.trigger) ? r.trigger : "manual";
  const [route, tab] = system
    ? (["cleanup", "system"] as const)
    : (TRIGGER_ROUTE[trigger] ?? ["cleanup"]);
  return {
    id: `cleanup:${r.at}:${i}`,
    at: r.at,
    route,
    tab,
    title: [
      system ? "activity.run.cleanup.system" : (`activity.run.cleanup.${trigger}` as I18nKey),
    ],
    result: stopped
      ? ["activity.result.stopped"]
      : errors
        ? ["activity.result.freed_errors", { bytes: freed, count: errors }]
        : ["activity.result.freed", { bytes: freed }],
    tone: stopped ? "neutral" : errors ? "warning" : "success",
    ms: r.duration_ms || null,
  };
}

function batchResult(items: UpdateItem[], reboot: boolean, cancelled: boolean) {
  const ok = items.filter((x) => x.status === "ok").length;
  const failed = items.filter((x) => x.status === "failed").length;
  const p = { ok, total: items.length };
  const result: [I18nKey, Params] = cancelled
    ? ["activity.result.stopped_at", p]
    : reboot && ok > 0
      ? ["activity.result.reboot", p]
      : ["activity.result.updated", p];
  const tone: Tone = cancelled ? "neutral" : failed ? (ok ? "warning" : "danger") : "success";
  return { result, tone };
}

/** Consecutive profile steps (same source, under a minute apart) are one run. */
function profileRuns(h: HistoryEntry[]): Run[] {
  const sorted = [...h].sort((a, b) => a.at - b.at);
  const groups: HistoryEntry[][] = [];
  for (const e of sorted) {
    const g = groups[groups.length - 1];
    const last = g?.[g.length - 1];
    if (g && last && last.source === e.source && e.at - last.at <= 60) g.push(e);
    else groups.push([e]);
  }
  return groups.map((g, i) => {
    const first = g[0] as HistoryEntry;
    const ok = g.filter((x) => x.ok).length;
    const special: Record<string, I18nKey> = {
      "@undo": "activity.run.profile.undo",
      "@undo_all": "activity.run.profile.undo_all",
      "@periph": "activity.run.profile.periph",
    };
    const key = special[first.source];
    return {
      id: `profile:${first.at}:${i}`,
      at: (g[g.length - 1] as HistoryEntry).at,
      route: first.source === "@periph" ? "peripherals" : "profiles",
      tab: first.source.startsWith("@undo") ? "restore" : undefined,
      title: key ? [key] : ["activity.run.profile", { name: first.source }],
      result: ["activity.result.steps", { ok, total: g.length }],
      tone: ok === g.length ? "success" : ok ? "warning" : "danger",
      ms: null,
    } satisfies Run;
  });
}

/** Every history, newest first (by when it ended), capped. Missing (null) histories are just skipped. */
export function normalizeRuns(h: Histories): Run[] {
  const runs: Run[] = [
    ...(h.cleanup ?? []).map(cleanupRun),
    ...(h.repair ?? []).map(
      (r, i): Run => ({
        id: `repair:${r.at}:${i}`,
        at: r.at,
        route: "maintenance",
        title: [`activity.run.repair.${r.tool}`],
        result: [`maintenance.verdict.${r.verdict}`],
        tone: REPAIR_TONE[r.verdict] ?? "neutral",
        ms: r.duration_ms,
      }),
    ),
    ...(h.volumes ?? []).map(
      (v, i): Run => ({
        id: `volume:${v.at}:${i}`,
        at: v.at,
        route: "maintenance",
        title: [`activity.run.volume.${v.op}`, { letter: v.letter || "?" }],
        result: [`maint2.verdict.${v.verdict}`],
        tone: VOLUME_TONE[v.verdict] ?? "neutral",
        ms: v.duration_ms,
      }),
    ),
    ...(h.plans ?? []).map((p, i): Run => {
      const failed = p.steps.filter((s) => s.state === "failed").length;
      const issues = failed + p.steps.filter((s) => s.state === "attention").length;
      const stopped = p.steps.some((s) => s.state === "cancelled");
      return {
        id: `plan:${p.at}:${i}`,
        at: p.at,
        route: "maintenance",
        title: [`activity.run.plan.${p.plan}`],
        result: stopped
          ? ["activity.result.stopped"]
          : issues
            ? ["activity.result.issues", { count: issues }]
            : ["activity.result.all_ok"],
        tone: stopped ? "neutral" : failed ? "danger" : issues ? "warning" : "success",
        ms: p.duration_ms,
      };
    }),
    ...(h.fixes ?? []).map(
      (f, i): Run => ({
        id: `fix:${f.at}:${i}`,
        at: f.at,
        route: "maintenance",
        title: [`activity.run.fix.${f.fix}`],
        result: [f.ok ? "activity.result.done" : "activity.result.failed"],
        tone: f.ok ? "success" : "danger",
        ms: f.duration_ms,
      }),
    ),
    ...(h.updates ?? []).map(
      (b, i): Run => ({
        id: `updates:${b.at}:${i}`,
        at: b.at,
        route: "apps",
        tab: "updates",
        title: [
          b.kind === "windows" ? "activity.run.updates.windows" : "activity.run.updates.apps",
        ],
        ...batchResult(b.items, b.reboot, b.cancelled),
        ms: null,
      }),
    ),
    ...(h.installs ?? []).map(
      (b, i): Run => ({
        id: `install:${b.at}:${i}`,
        at: b.at,
        route: "apps",
        tab: "install",
        title: ["activity.run.install"],
        ...batchResult(b.items, b.reboot, b.cancelled),
        ms: null,
      }),
    ),
    ...profileRuns(h.profiles ?? []),
  ];
  // By the end: a weekly review sits above the steps it ran (they keep their own lines).
  const end = (r: Run) => r.at + (r.ms ?? 0) / 1000;
  return runs.sort((a, b) => end(b) - end(a)).slice(0, MAX_RUNS);
}

/** Convention shared with the core's lines: `bytes` is a size, `count`/`ok`/`total`/`failed`
 *  numbers, the rest literal text (React renders it as text, never HTML). */
export function formatParams(p: Params | undefined, locale: string): Params | undefined {
  if (!p) return p;
  const nf = new Intl.NumberFormat(locale);
  const out: Params = {};
  for (const [k, v] of Object.entries(p)) {
    const n = Number(v);
    out[k] =
      k === "bytes" && Number.isFinite(n)
        ? formatBytes(n, locale)
        : ["count", "ok", "total", "failed"].includes(k) && Number.isFinite(n)
          ? nf.format(n)
          : String(v);
  }
  return out;
}

/** The bell line in the current language. A key this build does not know reads generically. */
export function lineText(t: T, locale: string, e: ActivityEntry): string {
  return e.key in ptBR
    ? t(e.key as I18nKey, formatParams(e.args, locale))
    : t("activity.unknown", { kind: e.kind });
}

/** `agora`, `há 5 minutos`, then the clock time today, then the date. */
export function formatAgo(at: number, now: number, locale: string): string {
  const s = Math.max(0, Math.round(now / 1000 - at));
  const rtf = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  if (s < 45) return rtf.format(0, "second");
  if (s < 3600) return rtf.format(-Math.round(s / 60), "minute");
  const d = new Date(at * 1000);
  const sameDay = new Date(now).toDateString() === d.toDateString();
  return sameDay
    ? d.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" })
    : d.toLocaleString(locale, {
        day: "numeric",
        month: "short",
        hour: "2-digit",
        minute: "2-digit",
      });
}

/** `<1 s`, `42 s`, `3 min`, `1 h 05 min`. */
export function formatDuration(ms: number): string {
  if (ms < 1000) return "<1 s";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} s`;
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  return `${Math.floor(m / 60)} h ${String(m % 60).padStart(2, "0")} min`;
}

/** Day bucket for the timelines: 0 = today, 1 = yesterday, otherwise the day's start (ms). */
export function dayKey(at: number, now: number): number {
  const start = (ms: number) => new Date(ms).setHours(0, 0, 0, 0);
  const day = start(at * 1000);
  const today = start(now);
  if (day === today) return 0;
  if (day === start(now - 86_400_000)) return 1;
  return day;
}
