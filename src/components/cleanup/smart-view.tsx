import {
  BrushCleaning,
  Check,
  Copy,
  FileClock,
  FolderSearch,
  Gamepad2,
  HardDrive,
  Minus,
  PackageOpen,
  RotateCcw,
  ScanSearch,
  ShieldCheck,
  SlidersHorizontal,
  Square,
  Trash2,
} from "lucide-react";
import { type ReactNode, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { LivePanel } from "@/components/cleanup/live-panel";
import { QuickClean } from "@/components/cleanup/quick-clean";
import { openDisk, openUnowned } from "@/components/disk/intent";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { CountUp } from "@/components/ui/count-up";
import { ExeIcon } from "@/components/ui/exe-icon";
import { Badge, Button, Card, HelpHint } from "@/components/ui/primitives";
import { cleanupHint } from "@/data/catalogs";
import {
  backend,
  type CleanPlan,
  type CleanSource,
  type CleanupProgress,
  type DriveInfo,
  inTauri,
  type ReviewItem,
  type RunRecord,
  type SmartRun,
  type SourceEvent,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { sizeText, usePageData } from "@/lib/luli";
import { navigate, openOrphans } from "@/lib/nav";
import { cn, formatBytes } from "@/lib/utils";

/** Catalog category or Windows handler id → its name (both live in the same queue). */
export function useItemName() {
  const { t } = useI18n();
  return useCallback(
    (id: string) =>
      t((id.startsWith("sys_") ? `volcache.h.${id}` : `cleanup.cat.${id}`) as I18nKey),
    [t],
  );
}

/** Category id → its app's exe (real icon), asked once per session. */
let exesOnce: Promise<Record<string, string>> | null = null;
export function useAppExes(): Record<string, string> {
  const [exes, setExes] = useState<Record<string, string>>({});
  useEffect(() => {
    exesOnce ??= backend.cleanAppExes().then((m) => m ?? {});
    let alive = true;
    exesOnce.then((m) => alive && setExes(m));
    return () => {
      alive = false;
    };
  }, []);
  return exes;
}

/** `mm:ss` since `since`, ticking every second. */
function useClock(since: number, on: boolean) {
  const [, tick] = useState(0);
  useEffect(() => {
    if (!on) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [on]);
  const s = Math.max(0, Math.floor((Date.now() - since) / 1000));
  return `${String(Math.floor(s / 60)).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

const SOURCES: { id: CleanSource; icon: ReactNode }[] = [
  { id: "catalog", icon: <BrushCleaning className="size-4" /> },
  { id: "system", icon: <ShieldCheck className="size-4" /> },
  { id: "big_old", icon: <FileClock className="size-4" /> },
  { id: "unowned", icon: <FolderSearch className="size-4" /> },
  { id: "leftovers", icon: <PackageOpen className="size-4" /> },
  { id: "dupes", icon: <Copy className="size-4" /> },
];

const BIG_OLD_QUERY = ">500mb >6m";
/** The vessel's ruler: a tick every 10 %, taller at 0, 50 and 100. */
const TICKS = [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100];

type Phase = "idle" | "analyzing" | "plan" | "confirm" | "running" | "done";

/**
 * The default cleanup screen: Analyze → every source measured live → Safe / Review / System →
 * one Clean (preview, token and history from the core) → the drive gaining space → next step.
 */
export function SmartView({
  elevated,
  autostart,
  onHistory,
  onDetailed,
}: {
  elevated: boolean;
  autostart: boolean;
  onHistory: (h: RunRecord[]) => void;
  onDetailed: (tab?: string) => void;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const name = useItemName();
  const exes = useAppExes();
  const [phase, setPhase] = useState<Phase>("idle");
  const [plan, setPlan] = useState<CleanPlan | null>(null);
  const [sources, setSources] = useState<Partial<Record<CleanSource, SourceEvent>>>({});
  const [live, setLive] = useState<CleanupProgress | null>(null);
  const [startedAt, setStartedAt] = useState(0);
  const [run, setRun] = useState<SmartRun | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [drive, setDrive] = useState<DriveInfo | null>(null);

  useEffect(() => {
    backend.listDrives().then((d) => {
      if (d) setDrive(d.find((x) => x.letter.toUpperCase().startsWith("C")) ?? d[0] ?? null);
    });
    const offs = [
      backend.on<SourceEvent>("smart:source", (e) => setSources((s) => ({ ...s, [e.source]: e }))),
      backend.on<CleanupProgress>("smart:progress", setLive),
    ];
    return () => {
      for (const p of offs) p.then((off) => off());
    };
  }, []);

  const analyze = useCallback(async () => {
    setPhase("analyzing");
    setSources({});
    setError(null);
    setRun(null);
    setStartedAt(Date.now());
    try {
      const p = await backend.smartAnalyze();
      setPlan(p);
      if (p.drive) setDrive(p.drive);
      setPhase("plan");
    } catch (e) {
      setError(errorText(e));
      setPhase("idle");
    }
  }, [errorText]);
  const started = useRef(false);
  useEffect(() => {
    if ((autostart || boot.autorun) && inTauri && !started.current) {
      started.current = true;
      void analyze();
    }
  }, [autostart, analyze]);

  const ids = plan ? [...plan.safe, ...(elevated ? plan.system : [])] : [];
  const cleanBytes = plan ? plan.safe_bytes + (elevated ? plan.system_bytes : 0) : 0;
  const sizeOf = useMemo(() => {
    const m = new Map<string, number>();
    for (const i of plan?.catalog ?? []) m.set(i.id, i.bytes);
    for (const h of plan?.handlers ?? []) m.set(h.id, h.bytes);
    return m;
  }, [plan]);

  const execute = async () => {
    setPhase("running");
    setLive({ done: 0, total: ids.length, current: null, bytes: 0, files: 0 });
    setStartedAt(Date.now());
    setError(null);
    try {
      const tok = await backend.smartConfirm(ids);
      const r = await backend.smartExecute(tok.token, ids);
      setRun(r);
      if (r.after) setDrive(r.after);
      setPhase("done");
      const h = await backend.cleanupHistory();
      if (h) onHistory(h);
    } catch (e) {
      setError(errorText(e));
      setPhase("plan");
    } finally {
      setLive(null);
    }
  };

  const luliData = useMemo(() => {
    if (!plan) return null;
    const rows = (list: string[]) =>
      list.map((id) => `  - ${id}: ${sizeText(sizeOf.get(id) ?? 0)}`).join("\n");
    return [
      `Smart cleanup analysis (drive ${plan.drive ? `${sizeText(plan.drive.free_bytes)} free of ${sizeText(plan.drive.total_bytes)}` : "unknown"}):`,
      `- safe, one click: ${sizeText(plan.safe_bytes)}\n${rows(plan.safe)}`,
      `- system (admin${plan.elevated ? "" : ", not elevated"}): ${sizeText(plan.system_bytes)}\n${rows(plan.system)}`,
      `- review: ${plan.review.map((r) => `${r.kind}${r.kind === "category" ? ` ${r.id}` : ""} ${sizeText(r.bytes)}`).join(", ") || "nothing"}`,
      `- not cleaned (big Windows items): ${plan.extras.map((x) => `${x.id} ${sizeText(x.bytes)}`).join(", ") || "none"}`,
    ].join("\n");
  }, [plan, sizeOf]);
  usePageData("cleanup", luliData);

  const found = Object.values(sources).reduce((s, e) => s + (e?.bytes ?? 0), 0);

  return (
    <>
      {phase === "idle" || phase === "analyzing" ? (
        <Card className="frame-ticks animate-rise flex flex-col gap-6 p-6">
          <DriveVessel
            drive={drive}
            junk={phase === "analyzing" ? found : 0}
            scanning={phase === "analyzing"}
          />
          {phase === "idle" ? (
            <div className="flex flex-wrap items-end justify-between gap-5">
              <div className="max-w-xl">
                <h2 className="font-display text-xl font-semibold tracking-wide">
                  {t("clean.idle.title")}
                </h2>
                <p className="mt-1.5 text-sm text-muted-foreground">{t("clean.idle.body")}</p>
                {!inTauri ? (
                  <p className="mt-2 text-xs text-warning">{t("clean.app_only")}</p>
                ) : null}
                {error ? <p className="mt-2 text-xs text-destructive">{error}</p> : null}
              </div>
              <div className="flex items-center gap-2">
                <Button variant="ghost" onClick={() => onDetailed()}>
                  <SlidersHorizontal className="size-4" />
                  {t("clean.mode.detailed")}
                </Button>
                <Button variant="primary" size="lg" tactile disabled={!inTauri} onClick={analyze}>
                  <ScanSearch className="size-4" />
                  {t("clean.analyze")}
                </Button>
              </div>
            </div>
          ) : (
            <Analyzing
              sources={sources}
              found={found}
              since={startedAt}
              elevated={elevated}
              nameOf={name}
            />
          )}
        </Card>
      ) : null}

      {plan && (phase === "plan" || phase === "confirm") ? (
        <>
          <Card className="frame-ticks animate-rise flex flex-col gap-6 p-6">
            <DriveVessel drive={drive} junk={cleanBytes} />
            <div className="flex flex-wrap items-end justify-between gap-5">
              <div>
                <p className="text-xs text-muted-foreground">{t("clean.plan.lead")}</p>
                <CountUp
                  value={cleanBytes}
                  format={(n) => formatBytes(n, locale)}
                  className="num font-display text-4xl font-semibold tracking-tight"
                />
                <p className="mt-1 text-xs text-muted-foreground">
                  {plan.review_bytes > 0
                    ? t("clean.plan.review_more", { size: formatBytes(plan.review_bytes, locale) })
                    : t("clean.plan.nothing_to_review")}
                </p>
                {error ? <p className="mt-2 text-xs text-destructive">{error}</p> : null}
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <Button variant="ghost" onClick={analyze}>
                  <RotateCcw className="size-4" />
                  {t("clean.again")}
                </Button>
                <Button
                  variant="primary"
                  size="lg"
                  tactile
                  disabled={ids.length === 0 || phase === "confirm"}
                  onClick={() => setPhase("confirm")}
                >
                  <BrushCleaning className="size-4" />
                  {cleanBytes > 0
                    ? t("clean.go", { size: formatBytes(cleanBytes, locale) })
                    : t("clean.go_plain")}
                </Button>
              </div>
            </div>
            {phase === "confirm" ? (
              <div className="rounded-md border border-warning/40 bg-warning/5 p-4">
                <p className="flex items-center gap-2 text-sm font-semibold">
                  <Trash2 className="size-4 text-warning" />
                  {t("clean.confirm.title", {
                    size: formatBytes(cleanBytes, locale),
                    count: ids.length,
                  })}
                </p>
                <p className="mt-1.5 text-xs text-muted-foreground">{t("clean.confirm.body")}</p>
                <div className="mt-3 flex justify-end gap-2">
                  <Button variant="ghost" size="sm" onClick={() => setPhase("plan")}>
                    {t("common.cancel")}
                  </Button>
                  <Button variant="primary" size="sm" onClick={execute}>
                    <Trash2 className="size-3.5" />
                    {t("cleanup.confirm.go")}
                  </Button>
                </div>
              </div>
            ) : null}
          </Card>
          <div className="grid grid-cols-1 gap-4 xl:grid-cols-3">
            <Bucket
              index={1}
              tone="success"
              title={t("clean.bucket.safe")}
              hint={t("clean.bucket.safe.hint")}
              bytes={plan.safe_bytes}
            >
              <ItemList ids={plan.safe} sizeOf={sizeOf} exes={exes} plan={plan} />
              {plan.safe.length === 0 ? <Empty text={t("clean.bucket.empty")} /> : null}
            </Bucket>
            <Bucket
              index={2}
              tone="warning"
              title={t("clean.bucket.review")}
              hint={t("clean.bucket.review.hint")}
              bytes={plan.review_bytes}
            >
              <ul className="flex flex-col divide-y divide-border">
                {plan.review.map((r) => (
                  <ReviewRow
                    key={r.kind === "category" ? r.id : r.kind}
                    item={r}
                    plan={plan}
                    elevated={elevated}
                    exes={exes}
                  />
                ))}
              </ul>
              {plan.review.length === 0 ? <Empty text={t("clean.bucket.empty")} /> : null}
            </Bucket>
            <Bucket
              index={3}
              tone="neutral"
              title={t("clean.bucket.system")}
              hint={t("clean.bucket.system.hint")}
              bytes={plan.system_bytes}
              footer={
                <div className="flex flex-wrap items-center gap-2">
                  {elevated ? (
                    <span className="text-xs text-muted-foreground">
                      {t("clean.bucket.system.included")}
                    </span>
                  ) : (
                    <>
                      <ElevateButton route="cleanup" tab="analyze" />
                      <span className="text-xs text-muted-foreground">
                        {t("clean.bucket.system.elevate")}
                      </span>
                    </>
                  )}
                  <Button
                    variant="ghost"
                    size="sm"
                    className="ml-auto"
                    onClick={() => onDetailed("system")}
                  >
                    {t("clean.bucket.system.more")}
                  </Button>
                </div>
              }
            >
              <ItemList
                ids={plan.system}
                sizeOf={sizeOf}
                exes={exes}
                plan={plan}
                unmeasured={elevated ? undefined : t("clean.bucket.system.unmeasured")}
              />
              {plan.system.length === 0 ? <Empty text={t("clean.bucket.empty")} /> : null}
            </Bucket>
          </div>
        </>
      ) : null}

      {phase === "running" && live ? (
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
          <Card className="frame-ticks animate-rise flex flex-col justify-between gap-6 p-6">
            <DriveVessel drive={drive} junk={cleanBytes} freed={live.bytes} />
            <div>
              <p className="text-xs text-muted-foreground">
                {t("clean.run.free_now", { drive: drive?.letter ?? "C:" })}
              </p>
              <CountUp
                value={(drive?.free_bytes ?? 0) + live.bytes}
                format={(n) => formatBytes(n, locale)}
                className="num font-display text-4xl font-semibold tracking-tight text-success"
              />
            </div>
          </Card>
          <LivePanel
            live={live}
            running
            ids={ids}
            since={startedAt}
            expected={cleanBytes}
            nameOf={name}
            onStop={() => void backend.smartCancel()}
            opaque={(id) =>
              id.startsWith("sys_") || plan?.catalog.find((i) => i.id === id)?.measurable === false
            }
            noteOf={(id) => (id === "dism" ? "cleanup.live.component" : undefined)}
          />
        </div>
      ) : null}

      {phase === "done" && run ? (
        <Done run={run} plan={plan} onAgain={analyze} nameOf={name} />
      ) : null}
    </>
  );
}

/* ---------------- the drive as a vessel ---------------- */

/**
 * The system drive as one bar: used | what can be freed (hatched) | free. While analyzing the
 * hatch grows out of "used" as sources report; while cleaning it drains into "free" by the bytes
 * actually deleted. Widths glide only without reduced motion.
 */
export function DriveVessel({
  drive,
  junk,
  freed = 0,
  scanning = false,
}: {
  drive: DriveInfo | null;
  junk: number;
  freed?: number;
  scanning?: boolean;
}) {
  const { t, locale } = useI18n();
  const total = drive?.total_bytes ?? 0;
  const free = (drive?.free_bytes ?? 0) + freed;
  const pending = Math.max(0, Math.min(junk - freed, total - free));
  const used = Math.max(0, total - free - pending);
  const pct = (n: number) => (total > 0 ? (100 * n) / total : 0);
  const size = (n: number) => (total > 0 ? formatBytes(n, locale) : "—");
  const freePct = Math.round(pct(free));
  const label = t("clean.vessel.aria", {
    drive: drive?.letter ?? "C:",
    free: size(free),
    junk: size(pending),
    total: size(total),
  });
  return (
    <div>
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          <HardDrive className="size-4 text-muted-foreground" />
          {t("clean.vessel.title", { drive: drive?.letter ?? "C:" })}
        </span>
        <span className="num text-xs text-muted-foreground">
          {total > 0
            ? t("clean.vessel.free_pct", { pct: freePct, total: formatBytes(total, locale, 0) })
            : "—"}
        </span>
      </div>
      <div
        role="img"
        aria-label={label}
        className="mt-3 flex h-8 overflow-hidden rounded-md bg-foreground/5 ring-1 ring-border ring-inset"
      >
        <span
          className="h-full bg-foreground/20 motion-safe:transition-[width] motion-safe:duration-700 motion-safe:ease-out-expo"
          style={{ width: `${pct(used)}%` }}
        />
        <span
          className={cn(
            "h-full motion-safe:transition-[width] motion-safe:duration-700 motion-safe:ease-out-expo",
            pending > 0 && "min-w-1",
            scanning && "motion-safe:animate-pulse",
          )}
          style={{
            width: `${pct(pending)}%`,
            backgroundImage:
              "repeating-linear-gradient(135deg, hsl(var(--primary) / 0.9) 0 5px, hsl(var(--primary) / 0.45) 5px 10px)",
          }}
        />
        <span className="h-full flex-1 bg-success/15 shadow-[inset_2px_0_0_hsl(var(--success))]" />
      </div>
      <div className="relative h-2" aria-hidden>
        {TICKS.map((at) => (
          <span
            key={at}
            className={cn("absolute top-0 w-px bg-foreground/15", at % 50 === 0 ? "h-2" : "h-1")}
            style={{ left: `${at}%` }}
          />
        ))}
      </div>
      <dl className="mt-1 grid grid-cols-3 gap-3 text-xs">
        <Legend swatch="bg-foreground/20" label={t("clean.vessel.used")} value={size(used)} />
        <Legend
          swatch="bg-primary"
          label={t("clean.vessel.junk")}
          value={size(pending)}
          className="text-center"
        />
        <Legend
          swatch="bg-success"
          label={t("clean.vessel.free")}
          value={size(free)}
          className="text-right"
        />
      </dl>
    </div>
  );
}

function Legend({
  swatch,
  label,
  value,
  className,
}: {
  swatch: string;
  label: string;
  value: string;
  className?: string;
}) {
  return (
    <div className={className}>
      <dt className="inline-flex items-center gap-1.5 text-muted-foreground">
        <span className={cn("size-2 rounded-sm", swatch)} aria-hidden />
        {label}
      </dt>
      <dd className="num font-semibold text-foreground">{value}</dd>
    </div>
  );
}

/* ---------------- analysis: every source, live ---------------- */

function Analyzing({
  sources,
  found,
  since,
  elevated,
  nameOf,
}: {
  sources: Partial<Record<CleanSource, SourceEvent>>;
  found: number;
  since: number;
  elevated: boolean;
  nameOf: (id: string) => string;
}) {
  const { t, locale } = useI18n();
  const clock = useClock(since, true);
  const [stopping, setStopping] = useState(false);
  return (
    <div className="grid gap-6 lg:grid-cols-[minmax(0,15rem)_1fr]">
      <div className="flex flex-col gap-3">
        <span className="flex items-center gap-2 text-xs text-muted-foreground" role="status">
          <span
            aria-hidden
            className="size-2 rounded-full bg-primary motion-safe:animate-pulse-dot"
          />
          {t("clean.analyzing")}
          <span className="num">{clock}</span>
        </span>
        <CountUp
          value={found}
          format={(n) => formatBytes(n, locale)}
          className="num font-display text-4xl font-semibold tracking-tight"
        />
        <span className="text-xs text-muted-foreground">{t("clean.analyzing.found")}</span>
        <Button
          variant="outline"
          size="sm"
          className="self-start"
          disabled={stopping}
          onClick={() => {
            setStopping(true);
            void backend.smartCancel();
          }}
        >
          <Square className="size-3.5" />
          {t(stopping ? "cleanup.live.stopping" : "cleanup.live.stop")}
        </Button>
      </div>
      <ol className="flex flex-col" aria-label={t("clean.sources")}>
        {SOURCES.map((s, i) => {
          const e = sources[s.id];
          const state = e?.state ?? "pending";
          const note =
            state === "running" && e?.current
              ? t("clean.source.now", { item: nameOf(e.current) })
              : state === "skipped"
                ? t(`clean.source.${s.id}.skipped` as I18nKey)
                : t(`clean.source.${s.id}.hint` as I18nKey);
          return (
            <li key={s.id} className="relative flex gap-3 pb-3 last:pb-0">
              {i < SOURCES.length - 1 ? (
                <span
                  aria-hidden
                  className={cn(
                    "absolute top-6 bottom-0 left-[11px] w-px transition-colors duration-300",
                    state === "done" ? "bg-success/50" : "bg-border",
                  )}
                />
              ) : null}
              <span
                aria-hidden
                className={cn(
                  "relative z-10 flex size-6 shrink-0 items-center justify-center rounded-full border transition-colors duration-300",
                  state === "done" && "border-success/50 bg-success/15 text-success",
                  state === "running" && "border-primary/70 bg-primary/10 text-primary",
                  state === "skipped" && "border-dashed border-border-strong text-muted-foreground",
                  state === "pending" && "border-border text-muted-foreground/60",
                )}
              >
                {state === "done" ? (
                  <Check className="size-3.5" />
                ) : state === "skipped" ? (
                  <Minus className="size-3.5" />
                ) : (
                  s.icon
                )}
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex items-baseline gap-2">
                  <span
                    className={cn(
                      "text-sm font-semibold",
                      state === "pending" && "text-muted-foreground",
                    )}
                  >
                    {t(`clean.source.${s.id}` as I18nKey)}
                  </span>
                  {e && state !== "skipped" && (e.bytes > 0 || state === "done") ? (
                    <CountUp
                      value={e.bytes}
                      format={(n) => formatBytes(n, locale)}
                      className="num ml-auto text-xs font-semibold"
                    />
                  ) : null}
                </div>
                <p className="truncate text-[11px] text-muted-foreground">
                  {s.id === "system" && state === "skipped" && elevated
                    ? t("clean.source.system.busy")
                    : note}
                </p>
                {state === "running" ? (
                  <span className="relative mt-1.5 block h-0.5 overflow-hidden rounded-full bg-border">
                    <span className="absolute inset-y-0 w-1/3 animate-sweep rounded-full bg-primary/70" />
                  </span>
                ) : null}
              </div>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

/* ---------------- buckets ---------------- */

const TONE_DOT = {
  success: "bg-success",
  warning: "bg-warning",
  neutral: "bg-muted-foreground",
} as const;

function Bucket({
  index,
  tone,
  title,
  hint,
  bytes,
  children,
  footer,
}: {
  index: number;
  tone: keyof typeof TONE_DOT;
  title: string;
  hint: string;
  bytes: number;
  children: ReactNode;
  footer?: ReactNode;
}) {
  const { locale } = useI18n();
  return (
    <Card className="animate-rise flex flex-col gap-4" style={rise(index)}>
      <header className="flex items-start justify-between gap-3">
        <div>
          <h3 className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
            <span aria-hidden className={cn("size-2 rounded-full", TONE_DOT[tone])} />
            {title}
            <HelpHint text={hint} />
          </h3>
        </div>
        <CountUp
          value={bytes}
          format={(n) => formatBytes(n, locale)}
          className="num font-display text-xl font-semibold"
        />
      </header>
      <div className="flex-1">{children}</div>
      {footer ? <div className="border-t border-border pt-3">{footer}</div> : null}
    </Card>
  );
}

function Empty({ text }: { text: string }) {
  return <p className="py-4 text-center text-xs text-muted-foreground">{text}</p>;
}

/** Catalog categories / handlers with their app icon, size and what holds them open. */
function ItemList({
  ids,
  sizeOf,
  exes,
  plan,
  unmeasured,
}: {
  ids: string[];
  sizeOf: Map<string, number>;
  exes: Record<string, string>;
  plan: CleanPlan;
  /** Shown instead of a size for handlers not measured (no admin). */
  unmeasured?: string;
}) {
  const { t, locale } = useI18n();
  const name = useItemName();
  const sorted = [...ids].sort((a, b) => (sizeOf.get(b) ?? 0) - (sizeOf.get(a) ?? 0));
  return (
    <ul className="flex flex-col gap-2 text-xs">
      {sorted.map((id) => {
        const p = plan.catalog.find((i) => i.id === id);
        const handler = id.startsWith("sys_");
        const bytes = sizeOf.get(id) ?? 0;
        return (
          <li key={id} className="flex items-center gap-2.5">
            <ExeIcon path={exes[id]} name={name(id)} className="size-4" />
            <span className="min-w-0 truncate">{name(id)}</span>
            {p?.blocked_by.length ? (
              <span title={t("cleanup.preview.blocked", { apps: p.blocked_by.join(", ") })}>
                <Badge tone="warning">{t("clean.in_use")}</Badge>
              </span>
            ) : null}
            <span className="num ml-auto shrink-0 text-muted-foreground">
              {handler && unmeasured
                ? unmeasured
                : p?.measurable === false
                  ? t("cleanup.preview.not_measurable")
                  : formatBytes(bytes, locale)}
            </span>
          </li>
        );
      })}
    </ul>
  );
}

/** One thing to look at, with the way to act on it (each with its own confirmation). */
function ReviewRow({
  item,
  plan,
  elevated,
  exes,
}: {
  item: ReviewItem;
  plan: CleanPlan;
  elevated: boolean;
  exes: Record<string, string>;
}) {
  const { t, locale } = useI18n();
  const name = useItemName();
  const size = formatBytes(item.bytes, locale);
  const row = (icon: ReactNode, title: string, body: string, action: ReactNode, help?: string) => (
    <li className="flex flex-col gap-2 py-3 first:pt-0 last:pb-0">
      <div className="flex items-center gap-2.5 text-xs">
        {icon}
        <span className="min-w-0 truncate font-semibold">{title}</span>
        {help ? <HelpHint text={help} /> : null}
        <span className="num ml-auto shrink-0 font-semibold">{size}</span>
      </div>
      <p className="text-[11px] text-muted-foreground">{body}</p>
      <div className="flex flex-wrap items-center gap-2">{action}</div>
    </li>
  );
  switch (item.kind) {
    case "category": {
      const p = plan.catalog.find((i) => i.id === item.id);
      return row(
        <ExeIcon path={exes[item.id]} name={name(item.id)} className="size-4" />,
        name(item.id),
        t(`clean.review.cat.${item.id === "recycle" ? "recycle" : "optin"}` as I18nKey),
        item.needs_admin && !elevated ? (
          <ElevateButton route="cleanup" tab="analyze" />
        ) : (
          <QuickClean ids={[item.id]} unmeasurable={p?.measurable === false} />
        ),
        t(cleanupHint(item.id)),
      );
    }
    case "big_old": {
      const names = plan.big_old
        .slice(0, 3)
        .map((f) => f.path.split("\\").pop())
        .join(", ");
      return row(
        <FileClock className="size-4 text-muted-foreground" />,
        t("clean.review.big_old"),
        t("clean.review.big_old.body", { count: item.count, names }),
        <Button
          variant="outline"
          size="sm"
          onClick={() =>
            openDisk({ tab: "largest", query: BIG_OLD_QUERY, scan: plan.drive?.letter ?? "C:" })
          }
        >
          <FolderSearch className="size-3.5" />
          {t("clean.review.big_old.go")}
        </Button>,
      );
    }
    case "unowned":
      return row(
        <FolderSearch className="size-4 text-muted-foreground" />,
        t("clean.review.unowned"),
        t("clean.review.unowned.body", { count: item.count, high: item.high }),
        <Button variant="outline" size="sm" onClick={openUnowned}>
          <PackageOpen className="size-3.5" />
          {t("clean.review.in_apps")}
        </Button>,
      );
    case "leftovers":
      return row(
        <PackageOpen className="size-4 text-muted-foreground" />,
        t("clean.review.leftovers"),
        t("clean.review.leftovers.body", { count: item.apps }),
        <Button
          variant="outline"
          size="sm"
          onClick={() => {
            openOrphans();
            navigate("apps");
          }}
        >
          <PackageOpen className="size-3.5" />
          {t("clean.review.in_apps")}
        </Button>,
      );
    case "dupes":
      return row(
        <Copy className="size-4 text-muted-foreground" />,
        t("clean.review.dupes"),
        t("clean.review.dupes.body", { count: item.groups }),
        <Button variant="outline" size="sm" onClick={() => openDisk({ tab: "dupes" })}>
          <Copy className="size-3.5" />
          {t("clean.review.dupes.go")}
        </Button>,
      );
  }
}

/* ---------------- after the run ---------------- */

function Done({
  run,
  plan,
  onAgain,
  nameOf,
}: {
  run: SmartRun;
  plan: CleanPlan | null;
  onAgain: () => void;
  nameOf: (id: string) => string;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const items = run.records.flatMap((r) => r.items);
  const freed = items.reduce((s, i) => s + i.bytes_freed, 0);
  const secs = run.records.reduce((s, r) => s + r.duration_ms, 0) / 1000;
  const before = run.before?.free_bytes ?? 0;
  const after = run.after?.free_bytes ?? 0;
  const drive = run.after?.letter ?? "C:";
  const review = (plan?.review ?? []).filter((r) => r.kind !== "category");
  const extras = (plan?.extras ?? []).filter((x) => x.id !== "swapfile");
  return (
    <>
      <Card className="frame-ticks animate-rise flex flex-col gap-6 p-6">
        <DriveVessel drive={run.after} junk={0} />
        <div className="flex flex-wrap items-end justify-between gap-5">
          <div>
            <p className="text-xs text-muted-foreground">{t("clean.done.lead")}</p>
            <CountUp
              value={freed}
              format={(n) => formatBytes(n, locale)}
              className="num font-display text-4xl font-semibold tracking-tight text-success"
            />
            {run.before && run.after ? (
              <p className="num mt-1 text-xs text-muted-foreground">
                {t("clean.done.before_after", {
                  drive,
                  before: formatBytes(before, locale),
                  after: formatBytes(after, locale),
                })}
                {after > before
                  ? ` · ${t("clean.done.gained", { size: formatBytes(after - before, locale) })}`
                  : ""}
                {` · ${t("cleanup.history.secs", { secs: secs.toFixed(1) })}`}
              </p>
            ) : null}
          </div>
          <Button variant="outline" onClick={onAgain}>
            <RotateCcw className="size-4" />
            {t("clean.again")}
          </Button>
        </div>
        <details className="text-xs">
          <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
            {t("clean.done.details", { count: items.length })}
          </summary>
          <ul className="mt-2 grid grid-cols-1 gap-1 md:grid-cols-2">
            {items.map((i) => (
              <li key={i.id} className="flex items-center gap-2">
                <span className="truncate">{nameOf(i.id)}</span>
                <span className="num ml-auto shrink-0 text-muted-foreground">
                  {i.error
                    ? i.error === "needs_admin"
                      ? t("cleanup.needs_admin")
                      : t("cleanup.result.error", { error: errorText(new Error(i.error)) })
                    : `${formatBytes(i.bytes_freed, locale)}${
                        i.skipped > 0
                          ? ` · ${t("cleanup.result.skipped", { count: i.skipped })}`
                          : ""
                      }`}
                </span>
              </li>
            ))}
          </ul>
        </details>
      </Card>

      <Card className="animate-rise flex flex-col gap-4" style={rise(1)}>
        <h3 className="font-display text-[15px] font-semibold tracking-wide">
          {run.tight
            ? t("clean.next.tight", { drive, free: formatBytes(after, locale) })
            : t("clean.next.ok", { drive, free: formatBytes(after, locale) })}
        </h3>
        <p className="-mt-2 text-xs text-muted-foreground">
          {run.tight ? t("clean.next.tight.body") : t("clean.next.ok.body")}
        </p>
        <div className="flex flex-wrap gap-2">
          {run.tight ? (
            <Button
              variant="primary"
              onClick={() => openDisk({ tab: "largest", query: BIG_OLD_QUERY, scan: drive })}
            >
              <FolderSearch className="size-4" />
              {t("clean.next.biggest")}
            </Button>
          ) : (
            <Button variant="outline" onClick={() => navigate("settings")}>
              <RotateCcw className="size-4" />
              {t("clean.next.schedule")}
            </Button>
          )}
          {review.map((r) =>
            r.kind === "unowned" ? (
              <Button key={r.kind} variant="outline" onClick={openUnowned}>
                <FolderSearch className="size-4" />
                {t("clean.next.unowned", { size: formatBytes(r.bytes, locale) })}
              </Button>
            ) : r.kind === "dupes" ? (
              <Button key={r.kind} variant="outline" onClick={() => openDisk({ tab: "dupes" })}>
                <Copy className="size-4" />
                {t("clean.next.dupes", { size: formatBytes(r.bytes, locale) })}
              </Button>
            ) : null,
          )}
        </div>
        {run.tight && extras.length > 0 ? (
          <ul className="grid grid-cols-1 gap-3 md:grid-cols-2">
            {extras.map((x) => (
              <li key={x.id} className="rounded-md border border-border p-3">
                <div className="flex items-center gap-2 text-xs">
                  {x.id === "games" ? (
                    <Gamepad2 className="size-4 text-muted-foreground" />
                  ) : (
                    <HardDrive className="size-4 text-muted-foreground" />
                  )}
                  <span className="font-semibold">
                    {x.id === "games"
                      ? t("clean.extra.games", { count: x.count })
                      : t(`disk.ess.${x.id}.title` as I18nKey)}
                  </span>
                  <span className="num ml-auto font-semibold">
                    {x.bytes > 0
                      ? formatBytes(x.bytes, locale)
                      : t("clean.extra.points", { count: x.count })}
                  </span>
                </div>
                <p className="mt-1.5 text-[11px] leading-relaxed text-muted-foreground">
                  {x.id === "games"
                    ? t("clean.extra.games.body")
                    : t(`disk.ess.${x.id}.body` as I18nKey)}
                </p>
                {x.id === "games" ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    className="mt-1.5"
                    onClick={() => navigate("games")}
                  >
                    <Gamepad2 className="size-3.5" />
                    {t("clean.extra.games.go")}
                  </Button>
                ) : null}
              </li>
            ))}
          </ul>
        ) : null}
      </Card>
    </>
  );
}
