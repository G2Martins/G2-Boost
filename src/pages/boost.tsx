import {
  Cpu,
  Eraser,
  Gauge as GaugeIcon,
  Keyboard,
  Lightbulb,
  Lock,
  Monitor,
  MousePointer2,
  Network,
  Plug,
  RefreshCw,
  Search,
  Server,
  Shield,
  ShieldCheck,
  Sparkles,
  Zap,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { PowerTab } from "@/components/boost/power-tab";
import { BootCard, PrivacyTab } from "@/components/boost/privacy-boot";
import { ProcsTab } from "@/components/boost/procs-tab";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { GameExesEditor } from "@/components/game-exes";
import { LuliCard } from "@/components/luli-card";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  HelpHint,
  Skeleton,
  Switch,
  selectClass,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import {
  categoryKey,
  FX_LEVELS,
  PRESET_FX_LEVEL,
  type Preset,
  reasonKey,
  TWEAKS,
  type Tweak,
  tweakDesc,
  tweakName,
  tweakWarn,
} from "@/data/catalogs";
import {
  backend,
  type DnsAdapter,
  type DnsRankEntry,
  type FxLevel,
  type GameEvent,
  inTauri,
  type MemoryStatus,
  type ProcGroup,
  type PurgeResult,
  type Recommendation,
  type RoutineStatus,
  type Settings,
  type StartupItem,
  type TweakState,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

const ICONS: Record<string, typeof Zap> = {
  visual_fx: Monitor,
  game_mode: Zap,
  input_lag: MousePointer2,
  windowed_optimizations: Monitor,
  keyboard: Keyboard,
  tips: Lightbulb,
  mouse: MousePointer2,
  svc_sysmain: Server,
  svc_telemetry: Shield,
  power_plan: Plug,
  hags: GaugeIcon,
  mmcss: Sparkles,
  network_latency: Network,
  svc_search: Search,
  svc_xbox: Server,
  priority_sep: Cpu,
  perf_counters: Cpu,
};

type StateMap = Record<string, TweakState>;

export function BoostPage() {
  const { t } = useI18n();
  const [elevated, setElevated] = useState(false);
  useEffect(() => {
    backend.appInfo().then((i) => i && setElevated(i.elevated));
  }, []);
  return (
    <PageHeaderWithTabs>
      <TabsContent value="tweaks">
        <PageBody className="px-0 pb-0">
          <LuliCard
            findings={["hvci_on", "cpu_hot", "gpu_hot", "refresh_below_max"]}
            asks={[{ label: "luli.ask.boost.worth" }, { label: "luli.ask.boost.hurt" }]}
          />
          <TweaksTab elevated={elevated} />
        </PageBody>
      </TabsContent>
      <TabsContent value="ram">
        <PageBody className="px-0 pb-0">
          <RamTab elevated={elevated} />
        </PageBody>
      </TabsContent>
      <TabsContent value="dns">
        <PageBody className="px-0 pb-0">
          <DnsTab elevated={elevated} />
        </PageBody>
      </TabsContent>
      <TabsContent value="startup">
        <PageBody className="px-0 pb-0">
          <BootCard elevated={elevated} />
          <StartupTab elevated={elevated} />
        </PageBody>
      </TabsContent>
      <TabsContent value="privacy">
        <PageBody className="px-0 pb-0">
          <PrivacyTab />
        </PageBody>
      </TabsContent>
      <TabsContent value="power">
        <PageBody className="px-0 pb-0">
          <PowerTab />
        </PageBody>
      </TabsContent>
      <TabsContent value="procs">
        <PageBody className="px-0 pb-0">
          <ProcsTab elevated={elevated} />
        </PageBody>
      </TabsContent>
      <span className="sr-only">{t("boost.title")}</span>
    </PageHeaderWithTabs>
  );
}

function PageHeaderWithTabs({ children }: { children: React.ReactNode }) {
  const { t } = useI18n();
  return (
    <PageHeader
      title={t("boost.title")}
      subtitle={t("boost.subtitle")}
      tabs={
        <Tabs
          defaultValue={
            ["ram", "dns", "startup", "privacy", "power", "procs"].includes(boot.startTab ?? "")
              ? (boot.startTab as string)
              : "tweaks"
          }
        >
          <TabsList>
            <TabsTrigger value="tweaks">{t("boost.tab.tweaks")}</TabsTrigger>
            <TabsTrigger value="ram">{t("boost.tab.ram")}</TabsTrigger>
            <TabsTrigger value="dns">{t("boost.tab.dns")}</TabsTrigger>
            <TabsTrigger value="startup">{t("boost.tab.startup")}</TabsTrigger>
            <TabsTrigger value="privacy">{t("boost.tab.privacy")}</TabsTrigger>
            <TabsTrigger value="power">{t("boost.tab.power")}</TabsTrigger>
            <TabsTrigger value="procs">{t("boost.tab.procs")}</TabsTrigger>
          </TabsList>
          {children}
        </Tabs>
      }
    />
  );
}

/** Which preset the live state matches exactly (checked from the largest set down). */
function activePreset(states: StateMap): Preset {
  for (const p of ["high", "default"] as const) {
    const ok = TWEAKS.every((tw) => {
      const s = states[tw.id];
      if (!s || s.status === "unavailable") return true;
      if (tw.id === "visual_fx") return s.level === PRESET_FX_LEVEL[p];
      return (s.status === "on") === tw.presets.includes(p);
    });
    if (ok) return p;
  }
  return "custom";
}

function TweaksTab({ elevated }: { elevated: boolean }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [states, setStates] = useState<StateMap | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [run, setRun] = useState<{ done: number; total: number } | null>(null);
  const [skipped, setSkipped] = useState(0);
  const [recs, setRecs] = useState<Record<string, Recommendation>>({});
  const [restoreBefore, setRestoreBefore] = useState(true);
  const [restoreMsg, setRestoreMsg] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    const list = await backend.boostState();
    if (list) setStates(Object.fromEntries(list.map((s) => [s.id, s])));
  }, []);
  useEffect(() => {
    refresh();
    // Changed something in Windows Settings meanwhile? Re-read when the window comes back.
    const onBack = () => document.visibilityState === "visible" && refresh();
    window.addEventListener("focus", onBack);
    document.addEventListener("visibilitychange", onBack);
    return () => {
      window.removeEventListener("focus", onBack);
      document.removeEventListener("visibilitychange", onBack);
    };
  }, [refresh]);
  useEffect(() => {
    backend
      .recommendations()
      .then((r) => r && setRecs(Object.fromEntries(r.items.map((x) => [x.id, x]))));
    backend.settingsGet().then((s) => s && setRestoreBefore(s.restore_before_preset));
  }, []);

  const put = (s: TweakState) => setStates((m) => ({ ...(m ?? {}), [s.id]: s }));

  /** One tweak on/off (visual effects take a level). Returns false on failure. */
  const setTweak = async (tw: Tweak, on: boolean, level?: FxLevel): Promise<boolean> => {
    setBusy(tw.id);
    setErrors((e) => ({ ...e, [tw.id]: "" }));
    try {
      const next =
        tw.id === "visual_fx"
          ? on
            ? await backend.boostVisualFx(level ?? "balanced")
            : await backend.boostVisualFxRevert()
          : await backend.boostApply(tw.id, on);
      put(next);
      return true;
    } catch (e) {
      setErrors((m) => ({ ...m, [tw.id]: errorText(e) }));
      return false;
    } finally {
      setBusy(null);
    }
  };

  const applyPreset = async (p: Preset) => {
    if (p === "custom" || !states) return;
    const todo = TWEAKS.filter((tw) => {
      const s = states[tw.id];
      if (!s || s.status === "unavailable") return false;
      if (tw.id === "visual_fx") return s.level !== PRESET_FX_LEVEL[p];
      return (s.status === "on") !== tw.presets.includes(p);
    });
    if (restoreBefore && elevated && todo.length > 0) {
      try {
        const out = await backend.restoreCreate(t("dash.restore.description"));
        setRestoreMsg(
          out === "created" ? t("boost.preset.restore_created") : t("boost.preset.restore_skipped"),
        );
      } catch (e) {
        setRestoreMsg(errorText(e));
      }
    }
    const runnable = todo.filter((tw) => !tw.admin || elevated);
    setSkipped(todo.length - runnable.length);
    setRun({ done: 0, total: runnable.length });
    let done = 0;
    for (const tw of runnable) {
      await setTweak(tw, tw.presets.includes(p), PRESET_FX_LEVEL[p]);
      done += 1;
      setRun({ done, total: runnable.length });
    }
    setRun(null);
  };

  const preset: Preset = states ? activePreset(states) : "custom";

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("boost.presets")}
          <HelpHint text={t("boost.tweaks.help")} />
        </span>
        <span className="flex items-center gap-2 text-xs text-muted-foreground">
          {elevated ? (
            <>
              <ShieldCheck className="size-4 text-success" />
              {t("boost.elevated")}
            </>
          ) : (
            <>
              <ElevateButton route="boost" />
              <HelpHint text={t("boost.elevate.hint")} />
            </>
          )}
        </span>
        <div className="ml-auto flex gap-2">
          {(["default", "high", "custom"] as Preset[]).map((p) => (
            <button
              key={p}
              type="button"
              disabled={!states || run !== null}
              onClick={() => applyPreset(p)}
              className={cn(
                "rounded-md border px-4 py-2.5 font-display text-[13px] font-semibold transition-colors disabled:opacity-50",
                preset === p
                  ? "border-ring bg-card-hover text-foreground"
                  : "border-border bg-background text-muted-foreground hover:text-foreground",
              )}
            >
              {t(`boost.preset.${p}` as const)}
            </button>
          ))}
        </div>
        {restoreMsg ? <p className="w-full text-xs text-muted-foreground">{restoreMsg}</p> : null}
        {run ? (
          <p className="w-full text-xs text-muted-foreground">
            {t("boost.preset.applying", { done: run.done, total: run.total })}
          </p>
        ) : null}
        {!run && skipped > 0 ? (
          <p className="w-full text-xs text-warning">
            {t("boost.preset.skipped_admin", { count: skipped })}
          </p>
        ) : null}
      </Card>
      {!inTauri ? <p className="text-xs text-muted-foreground">{t("common.soon")}</p> : null}
      <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
        {TWEAKS.map((tw, i) => (
          <TweakCard
            key={tw.id}
            tw={tw}
            index={i}
            state={states?.[tw.id]}
            loading={inTauri && states === null}
            elevated={elevated}
            busy={busy === tw.id || run !== null}
            error={errors[tw.id]}
            rec={recs[tw.id]}
            onToggle={(on, level) => setTweak(tw, on, level)}
          />
        ))}
      </div>
    </>
  );
}

function TweakCard({
  tw,
  index,
  state,
  loading,
  elevated,
  busy,
  error,
  rec,
  onToggle,
}: {
  tw: Tweak;
  index: number;
  state?: TweakState;
  loading: boolean;
  elevated: boolean;
  busy: boolean;
  error?: string;
  rec?: Recommendation;
  onToggle: (on: boolean, level?: FxLevel) => void;
}) {
  const { t } = useI18n();
  const Icon = ICONS[tw.id] ?? Zap;
  const locked = (tw.admin && !elevated) || !state || state.status === "unavailable";
  const on = state?.status === "on";
  const isFx = tw.id === "visual_fx";

  return (
    <Card className="panel-hover animate-rise flex flex-col gap-3" style={rise(index)}>
      <div className="flex items-start justify-between">
        <Icon className="size-6 text-foreground/90" strokeWidth={1.8} />
        {isFx ? null : (
          <Switch
            checked={on}
            disabled={locked || busy}
            onCheckedChange={(v) => onToggle(v)}
            aria-label={t(tweakName(tw.id))}
          />
        )}
      </div>
      <div>
        <h3 className="flex flex-wrap items-center gap-2 font-display text-[14px] font-semibold tracking-wide">
          {t(tweakName(tw.id))}
          {tw.beta ? <Badge tone="accent">{t("common.beta")}</Badge> : null}
        </h3>
        <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
          {t(tweakDesc(tw.id))}
        </p>
      </div>

      {isFx && state ? (
        <div className="flex flex-wrap items-center gap-1.5">
          {FX_LEVELS.map((lv) => (
            <button
              key={lv}
              type="button"
              disabled={busy}
              onClick={() => onToggle(true, lv)}
              className={cn(
                "press rounded-md border px-2.5 py-1.5 text-xs font-semibold transition-colors duration-200 ease-out-expo disabled:opacity-50",
                state.level === lv
                  ? "border-border-strong bg-foreground/8 text-foreground"
                  : "border-border text-muted-foreground hover:text-foreground",
              )}
            >
              {t(`boost.fx.level.${lv}` as const)}
            </button>
          ))}
          {state.level === null ? <Badge tone="warning">{t("boost.fx.custom")}</Badge> : null}
          {state.has_backup ? (
            <Button variant="ghost" size="sm" disabled={busy} onClick={() => onToggle(false)}>
              {t("boost.fx.revert")}
            </Button>
          ) : null}
        </div>
      ) : null}

      {state && state.services.length > 0 ? (
        <ul className="flex flex-col gap-1 text-[11px]">
          {state.services.map((s) => (
            <li key={s.name} className="flex items-center gap-2">
              <span
                className={cn(
                  "size-1.5 rounded-full",
                  s.running ? "bg-success" : "bg-muted-foreground/50",
                )}
                title={s.running ? t("boost.svc.running") : t("boost.svc.stopped")}
              />
              <span className="font-mono">{s.name}</span>
              <span className="text-muted-foreground">{t(`boost.svc.${s.start}` as const)}</span>
            </li>
          ))}
        </ul>
      ) : null}

      {state?.detail === "reboot_pending" ? (
        <p className="text-[11px] text-warning">{t("boost.state.reboot_pending")}</p>
      ) : state?.detail ? (
        <p className="text-[11px] text-muted-foreground">{state.detail}</p>
      ) : null}

      <details className="text-[11px]">
        <summary className="cursor-pointer text-muted-foreground underline decoration-dotted underline-offset-2 hover:text-foreground">
          {t("common.not_recommended")}
        </summary>
        <p className="mt-1 text-muted-foreground">{t(tweakWarn(tw.id))}</p>
      </details>

      {error ? <p className="text-xs text-destructive">{error}</p> : null}

      <div className="mt-auto flex flex-wrap items-center gap-2 text-[10px] text-muted-foreground">
        {loading ? (
          <>
            <Skeleton className="h-4 w-14" />
            <Skeleton className="h-4 w-20" />
          </>
        ) : null}
        {state ? (
          <Badge
            tone={
              state.status === "on" ? "success" : state.status === "partial" ? "warning" : "neutral"
            }
          >
            {t(`boost.state.${state.status}` as const)}
          </Badge>
        ) : null}
        {state ? <Badge tone="neutral">{t(categoryKey(state.category))}</Badge> : null}
        {rec && !rec.recommended && rec.reason ? (
          <Badge
            tone="warning"
            title={t("boost.not_recommended_here", { reason: t(reasonKey(rec.reason)) })}
          >
            {t(reasonKey(rec.reason))}
          </Badge>
        ) : null}
        {tw.admin ? <Badge tone="neutral">{t("common.admin_required")}</Badge> : null}
        {tw.reboot ? <Badge tone="neutral">{t("boost.state.reboot")}</Badge> : null}
        {state?.has_backup ? (
          <Badge tone="success" title={t("boost.state.backup.hint")}>
            {t("boost.state.backup")}
          </Badge>
        ) : state?.status === "on" ? (
          <Badge tone="neutral" title={t("boost.state.no_backup.hint")}>
            {t("boost.state.no_backup")}
          </Badge>
        ) : null}
      </div>
    </Card>
  );
}

function RamTab({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [mem, setMem] = useState<MemoryStatus | null>(null);
  const [last, setLast] = useState<{ at: Date; result: PurgeResult } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<RoutineStatus | null>(null);
  const [lastGame, setLastGame] = useState<GameEvent | null>(null);

  useEffect(() => {
    let alive = true;
    const tick = () => {
      backend.memoryStatus().then((m) => alive && m && setMem(m));
      backend.routineStatus().then((s) => alive && s && setStatus(s));
    };
    tick();
    backend.settingsGet().then((s) => alive && s && setSettings(s));
    const id = setInterval(tick, 3000);
    let off: (() => void) | null = null;
    backend.on<GameEvent>("game:started", setLastGame).then((u) => {
      off = u;
    });
    return () => {
      alive = false;
      clearInterval(id);
      off?.();
    };
  }, []);

  const save = async (patch: Partial<Settings>) => {
    if (!settings) return;
    setErr(null);
    try {
      setSettings(await backend.settingsSet({ ...settings, ...patch }));
    } catch (e) {
      setErr(errorText(e));
    }
  };

  const clean = async () => {
    setBusy(true);
    setErr(null);
    try {
      const result = await backend.ramPurge();
      setLast({ at: new Date(), result });
    } catch (e) {
      setErr(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const used = mem ? mem.total_bytes - mem.avail_bytes : 0;
  const pct = mem ? Math.round((used / mem.total_bytes) * 100) : 0;

  return (
    <>
      <Card className="animate-rise">
        <CardTitle
          help={t("boost.ram.help")}
          right={
            <span className="flex items-center gap-2">
              {!elevated ? <ElevateButton route="boost" /> : null}
              <Button variant="primary" disabled={busy || !inTauri} onClick={clean}>
                <Eraser className="size-4" />
                {t("boost.ram.clean")}
              </Button>
            </span>
          }
        >
          {t("boost.ram.title")}
          <span className="ml-2 font-sans text-xs font-normal text-muted-foreground">
            {last
              ? `${t("boost.ram.last", { when: last.at.toLocaleTimeString(locale) })} · ${
                  last.result.freed_bytes > 0
                    ? t("boost.ram.freed", { freed: formatBytes(last.result.freed_bytes, locale) })
                    : t("boost.ram.freed_none")
                }`
              : t("boost.ram.never")}
          </span>
        </CardTitle>
        {err ? <p className="mb-3 text-xs text-destructive">{err}</p> : null}
        <div className="h-5 w-full overflow-hidden rounded bg-border/60">
          <div
            className={cn(
              "h-full rounded transition-all duration-700",
              pct > 85 ? "bg-destructive" : "bg-chart-1",
            )}
            style={{ width: `${pct}%` }}
          />
        </div>
        <div className="mt-3 flex items-center gap-6 text-xs">
          <span className="flex items-center gap-2">
            <span className="size-3 rounded-sm bg-chart-1" />
            <b>{t("boost.ram.used")}</b>
            <span className="num text-muted-foreground">
              {mem ? formatBytes(used, locale) : "—"}
            </span>
          </span>
          <span className="flex items-center gap-2">
            <span className="size-3 rounded-sm border border-border" />
            <b>{t("boost.ram.free")}</b>
            <span className="num text-muted-foreground">
              {mem ? formatBytes(mem.avail_bytes, locale) : "—"}
            </span>
          </span>
          <span className="num ml-auto text-muted-foreground">
            {mem ? t("boost.ram.total", { total: formatBytes(mem.total_bytes, locale, 0) }) : "—"}
          </span>
        </div>
      </Card>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card className="animate-rise" style={rise(1)}>
          <CardTitle
            help={t("settings.game.ram.hint")}
            right={
              <Switch
                checked={!!settings?.game_ram_clean}
                disabled={!settings}
                onCheckedChange={(v) => save({ game_ram_clean: v })}
                aria-label={t("settings.game.ram")}
              />
            }
          >
            {t("settings.game.ram")}
          </CardTitle>
          <p className="text-xs text-muted-foreground">
            {status?.game
              ? t("settings.game.current", { name: status.game.name })
              : t("settings.game.none")}
          </p>
          {lastGame ? (
            <p className="mt-2 text-xs">
              {lastGame.error
                ? errorText(new Error(lastGame.error))
                : t("settings.game.last", {
                    name: lastGame.game.name,
                    freed: formatBytes(lastGame.freed_bytes, locale),
                  })}
            </p>
          ) : null}
          {!elevated ? (
            <p className="mt-3 flex flex-wrap items-center gap-2 text-xs text-warning">
              {t("settings.game.needs_admin")}
              <ElevateButton route="boost" />
            </p>
          ) : null}
          <p className="mt-4 rounded-md border border-chart-1/30 bg-chart-1/10 px-3 py-2.5 text-xs text-foreground/90">
            {t("boost.ram.notice")}
          </p>
        </Card>
        <Card className="animate-rise" style={rise(2)}>
          <CardTitle help={t("settings.game.exes.hint")}>{t("settings.game.exes")}</CardTitle>
          <GameExesEditor
            exes={settings?.game_exes ?? []}
            disabled={!settings}
            onChange={(game_exes) => save({ game_exes })}
          />
        </Card>
      </div>
    </>
  );
}

function DnsTab({ elevated }: { elevated: boolean }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [adapters, setAdapters] = useState<DnsAdapter[] | null>(null);
  const [rank, setRank] = useState<DnsRankEntry[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  // Multi-NIC / VPN: the user picks; default = the routed adapter (sorted first by the core).
  const [picked, setPicked] = useState<string | null>(null);

  const measure = useCallback(async () => {
    setBusy(true);
    const [a, r] = await Promise.all([backend.dnsAdapters(), backend.dnsRank(6)]);
    if (a) setAdapters(a);
    if (r) setRank(r);
    setBusy(false);
  }, []);
  useEffect(() => {
    void measure();
  }, [measure]);

  const main = adapters?.find((a) => a.guid === picked) ?? adapters?.[0] ?? null;
  const fastest = rank?.find((r) => r.median_ms !== null) ?? null;

  const apply = async (provider: string) => {
    if (!main) return;
    setMsg(null);
    try {
      await backend.dnsApply(main.guid, provider);
      setMsg(t("boost.dns.applied", { name: main.name }));
      setAdapters(await backend.dnsAdapters());
    } catch (e) {
      setMsg(errorText(e));
    }
  };
  const restore = async () => {
    if (!main) return;
    setMsg(null);
    try {
      await backend.dnsRestore(main.guid);
      setMsg(t("boost.dns.restored"));
      setAdapters(await backend.dnsAdapters());
    } catch (e) {
      setMsg(errorText(e));
    }
  };

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("boost.dns.title")}
          <HelpHint text={t("boost.dns.help")} />
        </span>
        {!elevated ? <ElevateButton route="boost" /> : null}
        <div className="ml-auto flex gap-2">
          <Button
            variant="outline"
            disabled={busy || !inTauri}
            onClick={measure}
            aria-label={t("boost.dns.rank")}
          >
            <RefreshCw className={cn("size-4", busy && "animate-spin")} />
          </Button>
          <Button
            variant="primary"
            disabled={!elevated || !fastest || !main || busy}
            onClick={() => fastest && apply(fastest.id)}
          >
            {t("boost.dns.pick_fastest")}
          </Button>
          <Button
            variant="outline"
            disabled={!elevated || !main?.has_backup || busy}
            onClick={restore}
          >
            {t("boost.dns.restore")}
          </Button>
        </div>
        {adapters && adapters.length > 1 ? (
          <label className="flex w-full items-center gap-2 text-xs text-muted-foreground">
            {t("boost.dns.adapter_pick")}
            <select
              value={main?.guid ?? ""}
              onChange={(e) => setPicked(e.target.value)}
              className={selectClass}
            >
              {adapters.map((a) => (
                <option key={a.guid} value={a.guid}>
                  {a.primary ? `${a.name} · ${t("boost.dns.routed")}` : a.name}
                </option>
              ))}
            </select>
          </label>
        ) : null}
        <p className="w-full text-xs text-muted-foreground">
          {main
            ? t("boost.dns.adapter", {
                name: main.name,
                servers: main.dns.join(", ") || "—",
                mode: main.static_dns.length > 0 ? t("boost.dns.static") : t("boost.dns.auto"),
              })
            : adapters
              ? t("boost.dns.no_adapter")
              : t("boost.dns.measuring")}
        </p>
        {msg ? <p className="w-full text-xs text-foreground/90">{msg}</p> : null}
      </Card>
      {rank === null && inTauri
        ? [0, 1, 2, 3].map((k) => (
            <Card key={k} className="flex items-center gap-4 py-4">
              <Skeleton className="size-9" />
              <Skeleton className="h-4 w-32" />
              <Skeleton className="ml-auto h-4 w-20" />
            </Card>
          ))
        : null}
      {(rank ?? []).map((r, i) => {
        const current = main?.provider === r.id;
        const isFastest = fastest?.id === r.id;
        return (
          <Card
            key={r.id}
            className="panel-hover animate-rise flex flex-wrap items-center gap-4 py-4"
            style={rise(i)}
          >
            <span className="num flex size-9 items-center justify-center rounded-md border border-border font-display font-semibold">
              {i + 1}
            </span>
            <span className="font-display text-[14px] font-semibold">{r.name}</span>
            <span className="num text-xs text-muted-foreground">
              {r.primary} / {r.secondary}
            </span>
            {current ? <Badge tone="success">{t("boost.dns.current")}</Badge> : null}
            {isFastest ? <Badge tone="accent">{t("boost.dns.fastest")}</Badge> : null}
            <span className="ml-auto text-xs text-muted-foreground">{t("boost.dns.avg")}</span>
            <span className="num w-20 text-right text-xs font-semibold">
              {r.median_ms === null ? t("boost.dns.timeout") : `${r.median_ms.toFixed(1)} ms`}
            </span>
            {r.loss_pct > 0 ? (
              <span className="num text-[11px] text-warning">
                {t("boost.dns.loss", { loss: Math.round(r.loss_pct) })}
              </span>
            ) : null}
            <Button
              variant="secondary"
              size="sm"
              disabled={!elevated || current || !main || busy}
              onClick={() => apply(r.id)}
            >
              {t("boost.dns.pick")}
            </Button>
          </Card>
        );
      })}
    </>
  );
}

/** Startup apps (Task Manager semantics) + what is heavy right now. Nothing is killed. */
function StartupTab({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [items, setItems] = useState<StartupItem[] | null>(null);
  const [procs, setProcs] = useState<ProcGroup[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    const [i, p] = await Promise.all([backend.startupList(), backend.procsTop()]);
    if (i) setItems(i);
    if (p) setProcs(p);
    setBusy(false);
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  const toggle = async (it: StartupItem, on: boolean) => {
    setMsg(null);
    try {
      const next = await backend.startupSet(it.location, it.name, on);
      setItems((cur) =>
        (cur ?? []).map((x) => (x.location === it.location && x.name === it.name ? next : x)),
      );
    } catch (e) {
      setMsg(errorText(e));
    }
  };

  const enabled = items?.filter((i) => i.enabled).length ?? 0;
  const ramMax = Math.max(1, ...(procs ?? []).map((p) => p.ram_bytes));

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("boost.startup.title")}
          <HelpHint text={t("boost.startup.help")} />
        </span>
        {items ? (
          <Badge tone="neutral">
            {t("boost.startup.count", { on: enabled, total: items.length })}
          </Badge>
        ) : null}
        {!elevated && items?.some((i) => i.needs_admin) ? <ElevateButton route="boost" /> : null}
        <Button
          variant="outline"
          className="ml-auto"
          disabled={busy || !inTauri}
          onClick={load}
          aria-label={t("boost.startup.refresh")}
        >
          <RefreshCw className={cn("size-4", busy && "animate-spin")} />
        </Button>
        <p className="w-full text-xs text-muted-foreground">{t("boost.startup.note")}</p>
        {msg ? <p className="w-full text-xs text-foreground/90">{msg}</p> : null}
      </Card>

      <div className="grid gap-3 xl:grid-cols-[1.25fr_1fr]">
        <Card className="animate-rise min-w-0 py-3" style={rise(1)}>
          <CardTitle className="mb-2">{t("boost.startup.apps")}</CardTitle>
          {items && items.length === 0 ? (
            <p className="py-6 text-center text-xs text-muted-foreground">
              {t("boost.startup.empty")}
            </p>
          ) : null}
          <ul className="divide-y divide-border">
            {items === null && inTauri
              ? [0, 1, 2, 3, 4].map((k) => (
                  <li key={k} className="flex items-center gap-3 py-3">
                    <div className="flex-1 space-y-1.5">
                      <Skeleton className="h-3.5 w-40" />
                      <Skeleton className="h-3 w-64" />
                    </div>
                    <Skeleton className="h-6 w-11 rounded-full" />
                  </li>
                ))
              : null}
            {(items ?? []).map((it) => {
              const locked = it.protected || (it.needs_admin && !elevated);
              return (
                <li key={`${it.location}:${it.name}`} className="flex items-center gap-3 py-2.5">
                  <div className="min-w-0 flex-1">
                    <p
                      className={cn(
                        "truncate text-sm font-medium",
                        !it.enabled && "text-muted-foreground line-through decoration-border",
                      )}
                    >
                      {it.name}
                    </p>
                    <p className="truncate text-[11px] text-muted-foreground" title={it.command}>
                      {t(`boost.startup.loc.${it.location}`)} · {it.command}
                    </p>
                  </div>
                  {it.protected ? (
                    <Badge tone="neutral">
                      <Lock className="mr-1 size-3" />
                      {t("boost.startup.security")}
                    </Badge>
                  ) : it.needs_admin && !elevated ? (
                    <Badge tone="neutral">{t("common.admin_required")}</Badge>
                  ) : null}
                  <Switch
                    checked={it.enabled}
                    disabled={locked}
                    onCheckedChange={(on) => toggle(it, on)}
                    aria-label={it.name}
                  />
                </li>
              );
            })}
          </ul>
        </Card>

        <Card className="animate-rise min-w-0 py-3" style={rise(2)}>
          <CardTitle className="mb-2 flex items-center gap-2">
            {t("boost.procs.title")}
            <HelpHint text={t("boost.procs.help")} />
          </CardTitle>
          <ul className="space-y-2">
            {procs === null && inTauri
              ? [0, 1, 2, 3, 4, 5].map((k) => (
                  <li key={k} className="space-y-1.5 py-1">
                    <Skeleton className="h-3.5 w-full" />
                    <Skeleton className="h-1 w-2/3" />
                  </li>
                ))
              : null}
            {(procs ?? []).map((p) => (
              <li key={p.name} className="text-sm">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium">{p.name}</span>
                  {p.count > 1 ? (
                    <span className="num text-[11px] text-muted-foreground">×{p.count}</span>
                  ) : null}
                  {p.system ? <Badge tone="neutral">{t("boost.procs.system")}</Badge> : null}
                  <span className="num ml-auto text-xs text-muted-foreground">
                    {p.cpu_pct.toFixed(1)}%
                  </span>
                  <span className="num w-20 text-right text-xs font-semibold">
                    {formatBytes(p.ram_bytes, locale)}
                  </span>
                </div>
                <div className="mt-1 h-1 overflow-hidden rounded-full bg-card-hover">
                  <div
                    className={cn(
                      "h-full rounded-full",
                      p.system ? "bg-muted-foreground/40" : "bg-chart-1",
                    )}
                    style={{ width: `${(p.ram_bytes / ramMax) * 100}%` }}
                  />
                </div>
              </li>
            ))}
          </ul>
          <p className="mt-3 text-[11px] text-muted-foreground">{t("boost.procs.note")}</p>
        </Card>
      </div>
    </>
  );
}
