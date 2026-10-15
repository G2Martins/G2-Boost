import {
  Cpu,
  Eraser,
  Gauge as GaugeIcon,
  Keyboard,
  Lightbulb,
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
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { GameExesEditor } from "@/components/game-exes";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  HelpHint,
  Switch,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import {
  DNS_PROVIDERS,
  FX_LEVELS,
  PRESET_FX_LEVEL,
  type Preset,
  TWEAKS,
  type Tweak,
  tweakDesc,
  tweakName,
  tweakWarn,
} from "@/data/catalogs";
import {
  backend,
  type FxLevel,
  type GameEvent,
  inTauri,
  type MemoryStatus,
  type PurgeResult,
  type RoutineStatus,
  type Settings,
  type TweakState,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

const ICONS: Record<string, typeof Zap> = {
  visual_fx: Monitor,
  game_mode: Zap,
  input_lag: MousePointer2,
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
          <DnsTab />
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
            boot.startTab === "ram" || boot.startTab === "dns" ? boot.startTab : "tweaks"
          }
        >
          <TabsList>
            <TabsTrigger value="tweaks">{t("boost.tab.tweaks")}</TabsTrigger>
            <TabsTrigger value="ram">{t("boost.tab.ram")}</TabsTrigger>
            <TabsTrigger value="dns">{t("boost.tab.dns")}</TabsTrigger>
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

  const refresh = useCallback(async () => {
    const list = await backend.boostState();
    if (list) setStates(Object.fromEntries(list.map((s) => [s.id, s])));
  }, []);
  useEffect(() => {
    refresh();
  }, [refresh]);

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
            elevated={elevated}
            busy={busy === tw.id || run !== null}
            error={errors[tw.id]}
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
  elevated,
  busy,
  error,
  onToggle,
}: {
  tw: Tweak;
  index: number;
  state?: TweakState;
  elevated: boolean;
  busy: boolean;
  error?: string;
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
          {tw.beta ? (
            <Badge className="bg-chart-1/20 text-chart-1">{t("common.beta")}</Badge>
          ) : null}
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
                "rounded-md border px-2.5 py-1.5 font-display text-[11px] font-semibold uppercase tracking-wider transition-colors disabled:opacity-50",
                state.level === lv
                  ? "border-ring bg-card-hover text-foreground"
                  : "border-border text-muted-foreground hover:text-foreground",
              )}
            >
              {t(`boost.fx.level.${lv}` as const)}
            </button>
          ))}
          {state.level === null ? (
            <Badge className="bg-warning/15 text-warning">{t("boost.fx.custom")}</Badge>
          ) : null}
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

      {state?.detail ? <p className="text-[11px] text-muted-foreground">{state.detail}</p> : null}

      <details className="text-[11px]">
        <summary className="cursor-pointer text-muted-foreground underline decoration-dotted underline-offset-2 hover:text-foreground">
          {t("common.not_recommended")}
        </summary>
        <p className="mt-1 text-muted-foreground">{t(tweakWarn(tw.id))}</p>
      </details>

      {error ? <p className="text-xs text-destructive">{error}</p> : null}

      <div className="mt-auto flex flex-wrap items-center gap-2 text-[10px] text-muted-foreground">
        {state ? (
          <Badge
            className={cn(
              state.status === "on" && "bg-success/15 text-success",
              state.status === "off" && "bg-card-hover text-muted-foreground",
              state.status === "partial" && "bg-warning/15 text-warning",
              state.status === "unavailable" && "bg-card-hover text-muted-foreground",
            )}
          >
            {t(`boost.state.${state.status}` as const)}
          </Badge>
        ) : null}
        {tw.admin ? (
          <Badge className="bg-card-hover text-muted-foreground">
            {t("common.admin_required")}
          </Badge>
        ) : null}
        {tw.reboot ? (
          <Badge className="bg-card-hover text-muted-foreground">{t("boost.state.reboot")}</Badge>
        ) : null}
        {state?.has_backup ? (
          <Badge className="bg-chart-1/15 text-chart-1" title={t("boost.state.backup.hint")}>
            {t("boost.state.backup")}
          </Badge>
        ) : state?.status === "on" ? (
          <Badge
            className="bg-card-hover text-muted-foreground"
            title={t("boost.state.no_backup.hint")}
          >
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

function DnsTab() {
  const { t } = useI18n();
  return (
    <>
      <Card className="animate-rise flex items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("boost.dns.title")}
          <HelpHint text={t("boost.dns.help")} />
        </span>
        <span className="text-xs text-muted-foreground">{t("boost.dns.soon")}</span>
        <div className="ml-auto flex gap-2">
          <Button
            variant="outline"
            disabled
            title={t("common.soon")}
            aria-label={t("boost.dns.rank")}
          >
            <RefreshCw className="size-4" />
          </Button>
          <Button variant="outline" disabled title={t("common.soon")}>
            {t("boost.dns.restore")}
          </Button>
        </div>
      </Card>
      {DNS_PROVIDERS.map((p, i) => (
        <Card
          key={p.id}
          className="panel-hover animate-rise flex items-center gap-4 py-4"
          style={rise(i)}
        >
          <span className="num flex size-9 items-center justify-center rounded-md border border-border font-display font-semibold">
            –
          </span>
          <span className="font-display text-[14px] font-semibold">{p.name}</span>
          <span className="num text-xs text-muted-foreground">
            {p.primary} / {p.secondary}
          </span>
          <span className="ml-auto text-xs text-muted-foreground">{t("boost.dns.avg")}</span>
          <span className="num w-16 text-xs font-semibold">—</span>
          <Button variant="secondary" size="sm" disabled title={t("common.soon")}>
            {t("boost.dns.pick")}
          </Button>
        </Card>
      ))}
    </>
  );
}
