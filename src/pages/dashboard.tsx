import {
  Activity,
  ArrowRight,
  BrushCleaning,
  Check,
  Cpu,
  Gauge as GaugeIcon,
  HardDrive,
  MemoryStick,
  Monitor,
  Network,
  Rocket,
  RotateCcw,
  ShieldCheck,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { AdvisorCard } from "@/components/advisor-card";
import { useErrorText } from "@/components/elevate-button";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Gauge } from "@/components/ui/gauge";
import { KpiCard } from "@/components/ui/kpi-card";
import { LiveLine } from "@/components/ui/live-line";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  Modal,
  Switch,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import { CLEANUP_CATALOG, categoryKey, TWEAKS, tweakName } from "@/data/catalogs";
import {
  type AppInfo,
  backend,
  inTauri,
  type MonitorSample,
  type Profile,
  type Recommendations,
  type RestoreStatus,
} from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { navigate } from "@/lib/nav";
import { cn, formatBytes } from "@/lib/utils";

const WINDOW = 60;
const DEFAULT_CLEANUP = CLEANUP_CATALOG.filter((c) => c.defaultOn).map((c) => c.id);
type Metric = "cpu" | "gpu" | "ram" | "disk" | "net";
const METRICS: { id: Metric; icon: typeof Cpu }[] = [
  { id: "cpu", icon: Cpu },
  { id: "gpu", icon: Monitor },
  { id: "ram", icon: MemoryStick },
  { id: "disk", icon: HardDrive },
  { id: "net", icon: Network },
];

export function DashboardPage() {
  const { t } = useI18n();
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    backend.appInfo().then(setInfo);
  }, []);
  const name = info?.user || "";
  return (
    <PageHeader
      title={t("dash.title")}
      subtitle={name ? t("dash.subtitle", { name }) : t("dash.subtitle_anon")}
      tabs={
        <PageBody className="px-0 pb-0">
          <Overview elevated={info?.elevated ?? false} />
        </PageBody>
      }
    />
  );
}

/* ================= Overview ================= */

function Overview({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [recs, setRecs] = useState<Recommendations | null>(null);
  const [restore, setRestore] = useState<RestoreStatus | null>(null);
  const [cleanupBytes, setCleanupBytes] = useState<number | null>(null);
  const [profile, setProfile] = useState<Profile | null>(null);
  const [samples, setSamples] = useState<MonitorSample[]>([]);
  const [metric, setMetric] = useState<Metric>("cpu");
  const [msg, setMsg] = useState<string | null>(null);
  const [boosting, setBoosting] = useState<{ done: number; total: number } | null>(null);
  const [skipped, setSkipped] = useState(0);
  const [busyRestore, setBusyRestore] = useState(false);
  const [busyRam, setBusyRam] = useState(false);
  const [restoreBefore, setRestoreBefore] = useState(true);
  const [confirmBoost, setConfirmBoost] = useState(false);
  const [boostClean, setBoostClean] = useState(true);
  const [boostRam, setBoostRam] = useState(true);

  const loadRecs = useCallback(async () => {
    const r = await backend.recommendations();
    if (r) setRecs(r);
  }, []);

  useEffect(() => {
    let alive = true;
    loadRecs();
    backend.restoreStatus().then((r) => alive && r && setRestore(r));
    backend.systemProfile().then((p) => alive && p && setProfile(p));
    backend.settingsGet().then((s) => alive && s && setRestoreBefore(s.restore_before_preset));
    backend
      .cleanupPreview(DEFAULT_CLEANUP)
      .then((items) => alive && setCleanupBytes(items.reduce((s, i) => s + i.bytes, 0)))
      .catch(() => alive && setCleanupBytes(null));
    let off: (() => void) | null = null;
    backend.monitorStart();
    backend
      .on<MonitorSample>("monitor:tick", (s) => {
        if (alive) setSamples((prev) => [...prev.slice(-(WINDOW - 1)), s]);
      })
      .then((u) => {
        off = u;
      });
    return () => {
      alive = false;
      off?.();
      backend.monitorStop();
    };
  }, [loadRecs]);

  const last = samples.at(-1) ?? null;
  const score = recs?.score ?? null;
  const pending = score?.pending ?? [];

  const boostNow = async (extras: { clean: boolean; ram: boolean }) => {
    if (!score) return;
    const todo = pending
      .map((id) => TWEAKS.find((tw) => tw.id === id))
      .filter((tw): tw is NonNullable<typeof tw> => !!tw);
    const runnable = todo.filter((tw) => !tw.admin || elevated);
    setSkipped(todo.length - runnable.length);
    setMsg(null);
    if (restoreBefore && elevated) {
      try {
        const out = await backend.restoreCreate(t("dash.restore.description"));
        setMsg(
          out === "created"
            ? t("dash.restore.created")
            : t("dash.restore.skipped", { minutes: restore?.frequency_minutes ?? 1440 }),
        );
      } catch (e) {
        setMsg(errorText(e));
      }
    }
    setBoosting({ done: 0, total: runnable.length });
    let done = 0;
    for (const tw of runnable) {
      try {
        if (tw.id === "visual_fx") await backend.boostVisualFx("balanced");
        else await backend.boostApply(tw.id, true);
      } catch (e) {
        setMsg(errorText(e));
      }
      done += 1;
      setBoosting({ done, total: runnable.length });
    }
    if (extras.clean) {
      try {
        const tok = await backend.cleanupConfirm(DEFAULT_CLEANUP);
        await backend.cleanupExecute(tok.token, DEFAULT_CLEANUP);
        const items = await backend.cleanupPreview(DEFAULT_CLEANUP);
        setCleanupBytes(items.reduce((sum, i) => sum + i.bytes, 0));
      } catch (e) {
        setMsg(errorText(e));
      }
    }
    if (extras.ram && elevated) await backend.ramPurge().catch(() => undefined);
    setBoosting(null);
    toast(t("dash.boost.done"), { tone: "success" });
    await loadRecs();
    backend.restoreStatus().then((r) => r && setRestore(r));
  };

  const createRestore = async () => {
    setBusyRestore(true);
    setMsg(null);
    try {
      const out = await backend.restoreCreate(t("dash.restore.description"));
      setMsg(
        out === "created"
          ? t("dash.restore.created")
          : t("dash.restore.skipped", { minutes: restore?.frequency_minutes ?? 1440 }),
      );
      const r = await backend.restoreStatus();
      if (r) setRestore(r);
    } catch (e) {
      const text =
        e instanceof Error && e.message === "protection_off"
          ? t("dash.restore.protection_off")
          : errorText(e);
      setMsg(text);
    } finally {
      setBusyRestore(false);
    }
  };

  const purge = async () => {
    setBusyRam(true);
    setMsg(null);
    try {
      const r = await backend.ramPurge();
      setMsg(
        r.freed_bytes > 0
          ? t("boost.ram.freed", { freed: formatBytes(r.freed_bytes, locale) })
          : t("boost.ram.freed_none"),
      );
    } catch (e) {
      setMsg(errorText(e));
    } finally {
      setBusyRam(false);
    }
  };

  const ramPct =
    last && last.ram_total > 0 ? Math.round((last.ram_used / last.ram_total) * 100) : null;
  const protection =
    restore?.enabled === true
      ? t("dash.restore.on")
      : restore?.enabled === false
        ? t("dash.restore.off")
        : t("dash.restore.unknown");

  return (
    <>
      {!inTauri ? <p className="text-xs text-muted-foreground">{t("common.soon")}</p> : null}
      {msg ? <p className="animate-rise text-xs text-foreground/90">{msg}</p> : null}
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-[minmax(0,1.2fr)_minmax(0,1fr)]">
        <Card className="animate-rise halo panel-hover flex flex-col items-center gap-5 sm:flex-row">
          <Gauge
            value={score?.percent ?? 0}
            max={100}
            label={
              score
                ? t(
                    score.percent >= 80
                      ? "dash.score.state.good"
                      : score.percent >= 50
                        ? "dash.score.state.ok"
                        : "dash.score.state.low",
                  )
                : t("dash.score.state.none")
            }
            valueText={score ? `${score.percent}%` : "—"}
            format={score ? (n) => `${Math.round(n)}%` : undefined}
            tone="download"
            className="frame-ticks shrink-0 p-2"
          />
          <div className="flex min-w-0 flex-1 flex-col gap-3 self-stretch sm:justify-center">
            <header>
              <p className="flex items-center gap-2 text-xs font-semibold text-muted-foreground">
                <GaugeIcon className="size-4 text-primary" />
                {t("dash.kpi.score")}
              </p>
              <p className="mt-1.5 text-[13px] leading-snug">
                {score
                  ? t("dash.kpi.score.caption", { active: score.active, total: score.recommended })
                  : t("dash.monitor.waiting")}
              </p>
            </header>
            {pending.length > 0 || (ramPct !== null && ramPct > 85) ? (
              <div className="flex flex-wrap gap-1.5">
                {pending.length > 0 ? (
                  <Badge tone="warning">
                    {t("dash.pending.tweaks", { count: pending.length })}
                  </Badge>
                ) : null}
                {ramPct !== null && ramPct > 85 ? (
                  <Badge tone="danger">{t("dash.pending.ram")}</Badge>
                ) : null}
              </div>
            ) : null}
            <Button
              variant="primary"
              size="lg"
              tactile
              className="self-start"
              disabled={!score || boosting !== null || (pending.length === 0 && !cleanupBytes)}
              onClick={() => setConfirmBoost(true)}
            >
              <Zap className="size-4" />
              {boosting
                ? t("dash.applying", { done: boosting.done, total: boosting.total })
                : !score
                  ? t("dash.boost.label")
                  : pending.length > 0
                    ? t("dash.kpi.score.action", { count: pending.length })
                    : cleanupBytes
                      ? t("dash.boost.label")
                      : t("dash.kpi.score.done")}
            </Button>
            {skipped > 0 && !boosting ? (
              <p className="text-[11px] text-warning">
                {t("dash.skipped_admin", { count: skipped })}
              </p>
            ) : null}
          </div>
        </Card>
        <Card className="animate-rise relative flex flex-col gap-4 overflow-hidden" style={rise(1)}>
          <div className="banner pointer-events-none absolute inset-0 opacity-70" aria-hidden />
          <div className="relative flex items-start gap-4">
            <span className="flex size-11 shrink-0 items-center justify-center rounded-xl border border-border-strong bg-background">
              <Rocket className="size-5 text-primary" />
            </span>
            <div className="min-w-0">
              <Badge tone="accent">{t("dash.setup.badge")}</Badge>
              <h3 className="mt-2 font-display text-[15px] font-semibold tracking-wide">
                {t("dash.setup.title")}
              </h3>
              <p className="mt-1 text-xs text-muted-foreground">{t("dash.setup.body")}</p>
            </div>
          </div>
          <ul className="relative grid gap-1.5 text-xs">
            {(
              [
                "dash.setup.point.apps",
                "dash.setup.point.debloat",
                "dash.setup.point.extras",
              ] as const
            ).map((k) => (
              <li key={k} className="flex items-center gap-2">
                <Check className="size-3.5 text-success" />
                {t(k)}
              </li>
            ))}
          </ul>
          <Button
            variant="outline"
            className="relative mt-auto self-start"
            onClick={() => navigate("setup")}
          >
            {t("dash.setup.go")}
            <ArrowRight className="size-4" />
          </Button>
        </Card>
      </div>
      <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
        <KpiCard
          icon={RotateCcw}
          label={t("dash.kpi.restore")}
          value="—"
          count={inTauri ? (restore?.count ?? null) : undefined}
          caption={
            <>
              {protection}
              {restore?.last ? ` · ${t("dash.kpi.restore.last", { when: restore.last })}` : ""}
            </>
          }
          action={
            elevated
              ? {
                  label: t("dash.kpi.restore.action"),
                  onClick: createRestore,
                  disabled: busyRestore || restore?.enabled === false,
                }
              : {
                  label: t("boost.elevate"),
                  onClick: () => backend.relaunchElevated("dashboard").catch(() => undefined),
                }
          }
          className="animate-rise"
          style={rise(1)}
        />
        <KpiCard
          icon={BrushCleaning}
          label={t("dash.kpi.cleanup")}
          value="—"
          count={inTauri ? cleanupBytes : undefined}
          format={(n) => formatBytes(n, locale)}
          caption={t("dash.kpi.cleanup.caption", { count: DEFAULT_CLEANUP.length })}
          action={{ label: t("dash.kpi.cleanup.action"), onClick: () => navigate("cleanup") }}
          className="animate-rise"
          style={rise(2)}
        />
        <KpiCard
          icon={MemoryStick}
          label={t("dash.kpi.ram")}
          value="—"
          count={inTauri ? ramPct : undefined}
          format={(n) => `${Math.round(n)}%`}
          tone={ramPct !== null && ramPct > 85 ? "danger" : "neutral"}
          caption={
            last
              ? t("dash.kpi.ram.caption", {
                  used: formatBytes(last.ram_used, locale),
                  total: formatBytes(last.ram_total, locale, 0),
                })
              : t("dash.monitor.waiting")
          }
          action={
            elevated
              ? { label: t("dash.kpi.ram.action"), onClick: purge, disabled: busyRam }
              : {
                  label: t("boost.elevate"),
                  onClick: () => backend.relaunchElevated("dashboard").catch(() => undefined),
                }
          }
          className="animate-rise"
          style={rise(3)}
        >
          <div className="h-1.5 w-full overflow-hidden rounded bg-border/60">
            <div
              className={cn(
                "h-full rounded bg-chart-1 transition-[width] duration-700 ease-out-expo",
                ramPct !== null && ramPct > 85 && "bg-destructive",
              )}
              style={{ width: `${ramPct ?? 0}%` }}
            />
          </div>
        </KpiCard>
      </div>

      <Modal
        open={confirmBoost}
        onClose={() => setConfirmBoost(false)}
        title={t("dash.boost.confirm.title")}
        closeLabel={t("common.close")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirmBoost(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                setConfirmBoost(false);
                void boostNow({ clean: boostClean && !!cleanupBytes, ram: boostRam });
              }}
            >
              <Zap className="size-4" />
              {t("dash.boost.confirm.go")}
            </Button>
          </>
        }
      >
        {pending.length > 0 ? (
          <>
            <p>{t("dash.boost.confirm.body", { count: pending.length })}</p>
            <ul className="mt-2 grid gap-1 text-xs text-muted-foreground sm:grid-cols-2">
              {pending.slice(0, 8).map((id) => (
                <li key={id} className="flex items-center gap-2">
                  <Check className="size-3.5 shrink-0 text-success" />
                  {t(tweakName(id))}
                </li>
              ))}
            </ul>
            {pending.length > 8 ? (
              <p className="mt-1 text-xs text-muted-foreground">
                {t("dash.boost.confirm.more", { count: pending.length - 8 })}
              </p>
            ) : null}
          </>
        ) : null}
        <div className="mt-4 grid gap-2.5 text-xs">
          {cleanupBytes ? (
            <div className="flex items-center gap-2.5">
              <Switch
                checked={boostClean}
                onCheckedChange={setBoostClean}
                aria-label={t("dash.boost.clean", { size: formatBytes(cleanupBytes, locale) })}
              />
              {t("dash.boost.clean", { size: formatBytes(cleanupBytes, locale) })}
            </div>
          ) : null}
          {elevated ? (
            <div className="flex items-center gap-2.5">
              <Switch
                checked={boostRam}
                onCheckedChange={setBoostRam}
                aria-label={t("dash.boost.ram")}
              />
              {t("dash.boost.ram")}
            </div>
          ) : null}
          {elevated && restoreBefore ? (
            <p className="text-muted-foreground">{t("dash.boost.restore")}</p>
          ) : null}
        </div>
      </Modal>

      <MonitorCard samples={samples} metric={metric} setMetric={setMetric} profile={profile} />

      <AdvisorCard style={rise(5)} />

      <Card className="animate-rise" style={rise(5)}>
        <CardTitle
          help={t("dash.actions.hint")}
          right={
            <Button variant="ghost" size="sm" onClick={() => navigate("boost")}>
              {t("dash.actions.all")}
            </Button>
          }
        >
          <Zap className="size-4" />
          {t("dash.actions")}
        </CardTitle>
        {pending.length === 0 ? (
          <EmptyState
            icon={<ShieldCheck className="size-5" />}
            title={t("dash.actions.empty")}
            body={t("dash.actions.hint")}
          />
        ) : (
          <ul className="divide-y divide-border">
            {pending.slice(0, 6).map((id) => {
              const tw = TWEAKS.find((x) => x.id === id);
              const locked = !!tw?.admin && !elevated;
              return (
                <li key={id} className="flex items-center gap-3 py-2.5 text-sm">
                  <span className="font-medium">{t(tweakName(id))}</span>
                  <Badge tone="neutral">{t(categoryKey(categoryOf(id)))}</Badge>
                  {tw?.admin ? <Badge tone="neutral">{t("common.admin_required")}</Badge> : null}
                  <Button
                    variant="outline"
                    size="sm"
                    className="ml-auto"
                    disabled={locked || boosting !== null}
                    onClick={async () => {
                      try {
                        if (id === "visual_fx") await backend.boostVisualFx("balanced");
                        else await backend.boostApply(id, true);
                        await loadRecs();
                      } catch (e) {
                        setMsg(errorText(e));
                      }
                    }}
                  >
                    {t("common.apply")}
                  </Button>
                </li>
              );
            })}
          </ul>
        )}
      </Card>
    </>
  );
}

/** The catalog's category (a core test keeps it equal to the Rust one). */
function categoryOf(id: string): string {
  return TWEAKS.find((tw) => tw.id === id)?.category ?? "performance";
}

function MonitorCard({
  samples,
  metric,
  setMetric,
  profile,
}: {
  samples: MonitorSample[];
  metric: Metric;
  setMetric: (m: Metric) => void;
  profile: Profile | null;
}) {
  const { t, locale } = useI18n();
  const last = samples.at(-1) ?? null;
  const pct = (s: MonitorSample): number => {
    switch (metric) {
      case "cpu":
        return s.cpu_pct;
      case "gpu":
        return s.gpu_pct;
      case "ram":
        return s.ram_total > 0 ? (s.ram_used / s.ram_total) * 100 : 0;
      case "disk":
        return s.disk_pct;
      case "net":
        return (s.net_rx_bytes_s + s.net_tx_bytes_s) / (1024 * 1024); // MB/s
    }
  };
  const isRate = metric === "net";
  const values = samples.map(pct);
  const current = last ? pct(last) : 0;
  const valueText = last
    ? isRate
      ? `${current.toFixed(1)} MB/s`
      : `${Math.round(current)}%`
    : "—";
  const mhz = last?.cpu_mhz ?? 0;
  return (
    <Card className="animate-rise" style={rise(4)}>
      <CardTitle
        right={
          <div className="flex flex-wrap gap-1.5">
            {METRICS.map((m) => (
              <button
                key={m.id}
                type="button"
                onClick={() => setMetric(m.id)}
                className={cn(
                  "press flex items-center gap-1.5 rounded-md border px-2.5 py-1.5 text-xs font-semibold transition-colors duration-200 ease-out-expo",
                  metric === m.id
                    ? "border-border-strong bg-foreground/8 text-foreground"
                    : "border-border text-muted-foreground hover:text-foreground",
                )}
              >
                <m.icon className="size-3.5" />
                {t(`dash.monitor.${m.id}` as const)}
              </button>
            ))}
          </div>
        }
      >
        <Activity className="size-4" />
        {t("dash.monitor")}
        {profile ? (
          <span className="ml-2 font-sans text-xs font-normal text-muted-foreground">
            {profile.cpu}
          </span>
        ) : null}
      </CardTitle>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[220px_1fr]">
        <Gauge
          value={isRate ? Math.min(current, 125) : current}
          max={isRate ? 125 : 100}
          label={t(`dash.monitor.${metric}` as const)}
          valueText={valueText}
          format={last ? (n) => (isRate ? `${n.toFixed(1)} MB/s` : `${Math.round(n)}%`) : undefined}
          tone="download"
        />
        <div className="min-w-0">
          <LiveLine
            series={[{ values, color: "hsl(var(--chart-1))" }]}
            window={WINDOW}
            seq={last?.seq}
            max={isRate ? undefined : 100}
            format={isRate ? (v) => `${v.toFixed(0)}` : undefined}
            label={t(`dash.monitor.${metric}` as const)}
          />
        </div>
      </div>
      <div className="mt-3 grid grid-cols-2 gap-2 text-xs md:grid-cols-4">
        <Chip
          label={t("dash.monitor.clock")}
          value={mhz > 0 ? `${(mhz / 1000).toFixed(2)} GHz` : "—"}
        />
        <Chip
          label={t("dash.monitor.cores")}
          value={profile ? `${profile.cores_physical} / ${profile.cores_logical}` : "—"}
        />
        <Chip
          label={t("dash.monitor.gpu_temp")}
          value={last?.gpu_temp_c != null ? `${last.gpu_temp_c} °C` : "—"}
          sub={
            last
              ? `${Math.round(last.gpu_pct)}% · ${formatBytes(last.gpu_mem_bytes, locale, 1)} ${t("dash.monitor.vram")}`
              : undefined
          }
        />
        <Chip
          label={t("dash.monitor.cpu_power")}
          value={last?.cpu_power_w != null ? `${last.cpu_power_w.toFixed(0)} W` : "—"}
          sub={
            last?.cpu_temp_c != null
              ? t("dash.monitor.cpu_temp_value", { temp: Math.round(last.cpu_temp_c) })
              : t("dash.monitor.cpu_temp.na")
          }
        />
        <Chip
          label={t("dash.monitor.disk_rate")}
          value={last ? `${(last.disk_bytes_s / (1024 * 1024)).toFixed(1)} MB/s` : "—"}
        />
        <Chip
          label={t("dash.monitor.net")}
          value={
            last
              ? t("dash.monitor.net_rate", {
                  rx: formatBytes(last.net_rx_bytes_s, locale, 1),
                  tx: formatBytes(last.net_tx_bytes_s, locale, 1),
                })
              : "—"
          }
        />
        <Chip
          label={t("dash.monitor.ram")}
          value={last ? formatBytes(last.ram_used, locale) : "—"}
          sub={profile ? formatBytes(profile.ram_bytes, locale, 0) : undefined}
        />
        <Chip
          label={t("dash.monitor.gpu_clock")}
          value={last?.gpu_core_mhz != null ? `${last.gpu_core_mhz} MHz` : "—"}
          sub={last?.gpu_power_w != null ? `${last.gpu_power_w.toFixed(0)} W` : undefined}
        />
      </div>
    </Card>
  );
}

function Chip({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="rounded-md border border-border bg-background/60 px-3 py-2">
      <div className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
        {label}
      </div>
      <div className="num mt-0.5 truncate font-display text-sm font-semibold" title={value}>
        {value}
      </div>
      {sub ? <div className="num truncate text-[10px] text-muted-foreground">{sub}</div> : null}
    </div>
  );
}
