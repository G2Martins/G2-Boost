// What the app is doing right now and what it told the person: one module store (like the toast)
// fed from App boot, so it survives the page unmounting when the route changes.
//
// Jobs: a long command the UI started (seen through `watchCalls` from start to settle) or a scan
// that reports by events (disk, duplicates). Its progress events update %, bytes; the pill and
// the sidebar dot read it. Feed: the bell's lines (`activity:new`), newest first.
import type { Route } from "@/components/shell/app-shell";
import {
  type ActivityEntry,
  type AppUpdateProgress,
  backend,
  type CleanupProgress,
  type MaintPlanRunning,
  type RepairProgress,
  type ScanProgressEvent,
  type VolumeProgress,
  type WuProgress,
  watchCalls,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import type { I18nKey } from "@/lib/i18n";
import { navigate } from "@/lib/nav";

export type JobKind =
  | "cleanup"
  | "volcache"
  | "disk"
  | "dupes"
  | "repair"
  | "volume"
  | "plan"
  | "updates"
  | "wu"
  | "install";

export type Job = {
  kind: JobKind;
  route: Route;
  /** Tab the page opens on (`boot.startTab`) when the pill or a run is clicked. */
  tab?: string;
  label: I18nKey;
  /** 0..1, null when the core gives no total. */
  percent: number | null;
  bytes: number | null;
  started: number;
  state: "run" | "ok" | "err";
  /** The bell line that came with the end (shown by the pill instead of a separate toast). */
  note?: string;
  /** Last moments of a finished job: the pill plays its exit. */
  leaving?: boolean;
  /** Born from an event, not from a call this UI made (nothing settles it but a done event). */
  adopted?: boolean;
};

const KINDS: Record<JobKind, { route: Route; tab?: string; label: I18nKey }> = {
  cleanup: { route: "cleanup", label: "jobs.cleanup.run" },
  volcache: { route: "cleanup", tab: "system", label: "jobs.volcache.run" },
  disk: { route: "disk", label: "jobs.disk" },
  dupes: { route: "disk", tab: "dupes", label: "jobs.dupes" },
  repair: { route: "maintenance", label: "jobs.repair" },
  volume: { route: "maintenance", label: "jobs.volume" },
  plan: { route: "maintenance", label: "jobs.plan" },
  updates: { route: "apps", tab: "updates", label: "jobs.updates" },
  wu: { route: "apps", tab: "updates", label: "jobs.wu" },
  install: { route: "apps", tab: "install", label: "jobs.install" },
};

/** Commands that run until the job ends (start → settle). Scans that return at once and report by
 *  events (disk, duplicates) are not here: their `done` event ends them. */
const CALLS: Record<string, [JobKind, I18nKey]> = {
  cleanup_preview: ["cleanup", "jobs.cleanup.measure"],
  cleanup_execute: ["cleanup", "jobs.cleanup.run"],
  smart_analyze: ["cleanup", "jobs.cleanup.measure"],
  smart_preview: ["cleanup", "jobs.cleanup.measure"],
  smart_execute: ["cleanup", "jobs.cleanup.run"],
  volcache_preview: ["volcache", "jobs.volcache.measure"],
  volcache_execute: ["volcache", "jobs.volcache.run"],
  repair_run: ["repair", "jobs.repair"],
  volumes_run: ["volume", "jobs.volume"],
  maint_plan_run: ["plan", "jobs.plan"],
  updates_apps_run: ["updates", "jobs.updates"],
  updates_wu_install: ["wu", "jobs.wu"],
  install_run: ["install", "jobs.install"],
};

/** How long a finished job stays (check or cross) before the pill and the dot let go. */
export const DONE_MS = 2400;

const frac = (n: number, d: number) => (d > 0 ? Math.min(1, Math.max(0, n / d)) : null);
const pct = (p: number | null) => (p === null ? null : Math.min(1, Math.max(0, p / 100)));
const batch = (p: AppUpdateProgress) => frac(p.index + (p.status === "running" ? 0 : 1), p.total);

let jobs: Job[] = [];
let feed: ActivityEntry[] = [];
const subs = new Set<() => void>();
const emit = () => {
  for (const s of subs) s();
};
export const subscribe = (cb: () => void) => {
  subs.add(cb);
  return () => {
    subs.delete(cb);
  };
};
export const getJobs = () => jobs;
export const getFeed = () => feed;

type Ended = (job: Job) => void;
type Line = (entry: ActivityEntry) => void;
const ended = new Set<Ended>();
const lines = new Set<Line>();
/** The shell's effects (toasts, native notification) hear here; they hold i18n and the route. */
export function onJobEnded(f: Ended) {
  ended.add(f);
  return () => {
    ended.delete(f);
  };
}
export function onLine(f: Line) {
  lines.add(f);
  return () => {
    lines.delete(f);
  };
}

function patch(kind: JobKind, p: Partial<Job>) {
  jobs = jobs.map((j) => (j.kind === kind ? { ...j, ...p } : j));
  emit();
}

/** The page on screen (App keeps it current): a job belongs to the page that started it, which
 *  already shows its progress (the Painel's own measure, the post-format wizard's installs). */
let here: Route = "dashboard";
export function setHere(route: Route) {
  here = route;
}

function begin(kind: JobKind, label?: I18nKey, origin?: Route) {
  const cur = jobs.find((j) => j.kind === kind);
  if (cur?.state === "run") {
    if (label && cur.label !== label) patch(kind, { label });
    return;
  }
  const d = KINDS[kind];
  const route = origin ?? d.route;
  const job: Job = {
    kind,
    route,
    tab: route === d.route ? d.tab : undefined,
    label: label ?? d.label,
    percent: null,
    bytes: null,
    started: Date.now(),
    state: "run",
    adopted: origin === undefined,
  };
  jobs = [...jobs.filter((j) => j.kind !== kind), job];
  emit();
}

const endedAt = new Map<JobKind, number>();
const quiet = new Map<JobKind, number>();

function progress(kind: JobKind, percent: number | null, bytes: number | null = null) {
  const running = jobs.some((j) => j.kind === kind && j.state === "run");
  // A straggler event of the run that just ended must not start a job nobody will end.
  if (!running && Date.now() - (endedAt.get(kind) ?? 0) < 3000) return;
  begin(kind);
  patch(kind, { percent, bytes });
  // ponytail: an adopted job that goes quiet for 2 min is dropped (no call will settle it).
  window.clearTimeout(quiet.get(kind));
  quiet.set(
    kind,
    window.setTimeout(() => {
      const j = jobs.find((x) => x.kind === kind);
      if (j?.state !== "run" || !j.adopted) return;
      jobs = jobs.filter((x) => x !== j);
      emit();
    }, 120_000),
  );
}

function end(kind: JobKind, ok: boolean) {
  const j = jobs.find((x) => x.kind === kind);
  if (j?.state !== "run") return;
  endedAt.set(kind, Date.now());
  patch(kind, { state: ok ? "ok" : "err", percent: ok ? 1 : j.percent });
  const done = jobs.find((x) => x.kind === kind);
  if (done) for (const f of ended) f(done);
  // Only this run: a new job of the same kind may have started meanwhile.
  const same = (x: Job) => x.kind === kind && x.started === j.started;
  window.setTimeout(() => {
    jobs = jobs.map((x) => (same(x) ? { ...x, leaving: true } : x));
    emit();
  }, DONE_MS - 220);
  window.setTimeout(() => {
    jobs = jobs.filter((x) => !same(x));
    emit();
  }, DONE_MS);
}

/** Open a module's page, on a tab when it has one (the page reads `boot.startTab` on mount). */
export function goTo(route: Route, tab?: string) {
  boot.startTab = tab;
  navigate(route);
}

/** Attach the bell line to the job of that route (running or just ended); false = no such job. */
export function attachNote(route: string | null, text: string): boolean {
  const j = jobs.find((x) => x.route === route);
  if (!j) return false;
  patch(j.kind, { note: text });
  return true;
}

export function markRead(ids: number[] = []) {
  const all = ids.length === 0;
  if (!feed.some((e) => !e.read && (all || ids.includes(e.id)))) return;
  feed = feed.map((e) => (all || ids.includes(e.id) ? { ...e, read: true } : e));
  emit();
  void backend.activityRead(ids);
}

export function clearFeed() {
  feed = [];
  emit();
  void backend.activityClear();
}

let started = false;
/** Once, at App boot: listen to every progress event and to the bell. */
export function startJobs() {
  if (started) return;
  started = true;
  watchCalls((cmd, settled) => {
    const c = CALLS[cmd];
    if (!c) return;
    const [kind, label] = c;
    begin(kind, label, here);
    settled.then(
      () => end(kind, true),
      () => end(kind, false),
    );
  });
  const cleanup = (kind: JobKind) => (p: CleanupProgress) => {
    if (p.current === null && p.done >= p.total && p.total > 0) {
      patch(kind, { bytes: p.bytes });
      end(kind, true);
    } else progress(kind, frac(p.done, p.total), p.bytes);
  };
  void backend.on<CleanupProgress>("cleanup:measure", cleanup("cleanup"));
  void backend.on<CleanupProgress>("cleanup:progress", cleanup("cleanup"));
  void backend.on<CleanupProgress>("smart:measure", cleanup("cleanup"));
  void backend.on<CleanupProgress>("smart:progress", cleanup("cleanup"));
  void backend.on<CleanupProgress>("volcache:progress", cleanup("volcache"));
  void backend.on<ScanProgressEvent>("disk:progress", (p) =>
    progress("disk", p.expected > 0 ? frac(p.bytes, p.expected) : null, p.bytes),
  );
  void backend.on("disk:done", () => end("disk", true));
  void backend.on("disk:error", () => end("disk", false));
  void backend.on<{ hashed_bytes: number }>("dupes:progress", (p) =>
    progress("dupes", null, p.hashed_bytes),
  );
  void backend.on("dupes:done", () => end("dupes", true));
  void backend.on<RepairProgress>("maintenance:progress", (p) =>
    progress("repair", pct(p.percent)),
  );
  void backend.on<VolumeProgress>("maintenance:volume-progress", (p) =>
    progress("volume", pct(p.percent)),
  );
  void backend.on<MaintPlanRunning>("maintenance:plan-progress", (p) =>
    progress("plan", frac(p.index + (pct(p.percent) ?? 0), p.total)),
  );
  void backend.on<AppUpdateProgress>("updates:apps", (p) => progress("updates", batch(p)));
  void backend.on<AppUpdateProgress>("install:progress", (p) => progress("install", batch(p)));
  void backend.on<WuProgress>("updates:wu", (p) => progress("wu", pct(p.percent)));
  // ponytail: no staleness sweep; every job here ends by its call settling or its done event.

  void backend.activityList().then((l) => {
    if (!l) return;
    // Lines that arrived while the list loaded stay on top.
    const live = new Set(feed.map((e) => e.id));
    feed = [...feed, ...l.filter((e) => !live.has(e.id)).reverse()];
    emit();
  });
  void backend.on<ActivityEntry>("activity:new", (e) => {
    feed = [e, ...feed.filter((x) => x.id !== e.id)].slice(0, 200);
    emit();
    for (const f of lines) f(e);
  });
}
