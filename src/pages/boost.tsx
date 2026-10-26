import { Eraser, Lock, RefreshCw, Search, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { PowerTab } from "@/components/boost/power-tab";
import { BootCard, PrivacyTab } from "@/components/boost/privacy-boot";
import { ProcsTab } from "@/components/boost/procs-tab";
import {
  RestartBadge,
  type StateMap,
  setTweak,
  TweakChanges,
  unknownNeedsAdmin,
  useTweakErrorText,
  useTweakStates,
} from "@/components/boost/tweak-kit";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { GameExesEditor } from "@/components/game-exes";
import { LuliCard } from "@/components/luli-card";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  HelpHint,
  Skeleton,
  Switch,
  selectClass,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import {
  categoryKey,
  FX_LEVELS,
  PRESET_FX_LEVEL,
  PRESET_MANAGED,
  type Preset,
  reasonKey,
  TWEAK_CATEGORIES,
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
  type TweakCategory,
  type TweakState,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

/** Case- and accent-insensitive search text. */
const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

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

/** Which preset the live state matches exactly (checked from the largest set down). Only preset
 *  members count: debloat and Windows tweaks are the user's own and never make it "custom". */
function activePreset(states: StateMap): Preset {
  for (const p of ["high", "default"] as const) {
    const ok = PRESET_MANAGED.every((tw) => {
      const s = states[tw.id];
      if (!s || s.status === "unavailable") return true;
      if (tw.id === "visual_fx") return s.level === PRESET_FX_LEVEL[p];
      return (s.status === "on") === tw.presets.includes(p);
    });
    if (ok) return p;
  }
  return "custom";
}

type Step = { tw: Tweak; on: boolean; level?: FxLevel };

function TweaksTab({ elevated }: { elevated: boolean }) {
  const { t } = useI18n();
  const errorText = useTweakErrorText();
  const { states, put } = useTweakStates();
  const [busy, setBusy] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [run, setRun] = useState<{ done: number; total: number } | null>(null);
  const [skipped, setSkipped] = useState(0);
  const [recs, setRecs] = useState<Record<string, Recommendation>>({});
  const [restoreBefore, setRestoreBefore] = useState(true);
  const [restoreMsg, setRestoreMsg] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<TweakCategory | "all">("all");

  useEffect(() => {
    backend
      .recommendations()
      .then((r) => r && setRecs(Object.fromEntries(r.items.map((x) => [x.id, x]))));
    backend.settingsGet().then((s) => s && setRestoreBefore(s.restore_before_preset));
  }, []);

  /** One tweak on/off (visual effects take a level). Returns false on failure. */
  const one = async ({ tw, on, level }: Step): Promise<boolean> => {
    setBusy(tw.id);
    setErrors((e) => ({ ...e, [tw.id]: "" }));
    try {
      put(await setTweak(tw.id, on, level));
      return true;
    } catch (e) {
      setErrors((m) => ({ ...m, [tw.id]: errorText(e) }));
      return false;
    } finally {
      setBusy(null);
    }
  };

  /** A card's own switch: toast, with undo when it turned something on. */
  const toggle = async (step: Step) => {
    if (!(await one(step))) return;
    const name = t(tweakName(step.tw.id));
    if (step.on) {
      toast(t("tweaks.toast.applied", { name }), {
        tone: "success",
        action: { label: t("tweaks.toast.undo"), run: () => void one({ tw: step.tw, on: false }) },
      });
    } else {
      toast(t("tweaks.toast.undone", { name }));
    }
  };

  /** Several in a row, after a restore point when the setting asks for one. */
  const runBatch = async (steps: Step[]): Promise<Tweak[]> => {
    if (steps.length === 0) return [];
    if (restoreBefore && elevated) {
      try {
        const out = await backend.restoreCreate(t("dash.restore.description"));
        setRestoreMsg(
          out === "created" ? t("boost.preset.restore_created") : t("boost.preset.restore_skipped"),
        );
      } catch (e) {
        setRestoreMsg(errorText(e));
      }
    }
    const done: Tweak[] = [];
    setRun({ done: 0, total: steps.length });
    for (const [i, step] of steps.entries()) {
      if (await one(step)) done.push(step.tw);
      setRun({ done: i + 1, total: steps.length });
    }
    setRun(null);
    return done;
  };

  const applyPreset = async (p: Preset) => {
    if (p === "custom" || !states) return;
    const todo = PRESET_MANAGED.filter((tw) => {
      const s = states[tw.id];
      if (!s || s.status === "unavailable") return false;
      if (tw.id === "visual_fx") return s.level !== PRESET_FX_LEVEL[p];
      return (s.status === "on") !== tw.presets.includes(p);
    });
    const runnable = todo.filter((tw) => !tw.admin || elevated);
    setSkipped(todo.length - runnable.length);
    await runBatch(
      runnable.map((tw) => ({ tw, on: tw.presets.includes(p), level: PRESET_FX_LEVEL[p] })),
    );
  };

  /** Recommended for this PC and not fully on yet (what the dashboard score counts). */
  const pending = TWEAKS.filter((tw) => {
    const s = states?.[tw.id];
    return recs[tw.id]?.recommended && s && s.status !== "on" && s.status !== "unavailable";
  });
  const pendingRunnable = pending.filter((tw) => !tw.admin || elevated);
  const applyRecommended = async () => {
    setSkipped(pending.length - pendingRunnable.length);
    const done = await runBatch(pendingRunnable.map((tw) => ({ tw, on: true })));
    if (done.length > 0) {
      toast(t("tweaks.toast.batch", { count: done.length }), {
        tone: "success",
        action: {
          label: t("tweaks.toast.undo"),
          run: () => void runBatch([...done].reverse().map((tw) => ({ tw, on: false }))),
        },
      });
    }
  };

  const q = fold(query.trim());
  const shown = TWEAKS.filter(
    (tw) =>
      (category === "all" || tw.category === category) &&
      (!q || fold(`${t(tweakName(tw.id))} ${t(tweakDesc(tw.id))} ${tw.id}`).includes(q)),
  );
  const present = shown.filter((tw) => {
    const s = states?.[tw.id];
    return s?.status !== "unavailable" || unknownNeedsAdmin(s);
  });
  const active = shown.filter((tw) => states?.[tw.id]?.status === "on").length;
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
              <ElevateButton route="boost" tab="tweaks" />
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
            {t("tweaks.applying", { done: run.done, total: run.total })}
          </p>
        ) : null}
        {!run && skipped > 0 ? (
          <p className="w-full text-xs text-warning">
            {t("boost.preset.skipped_admin", { count: skipped })}
          </p>
        ) : null}
      </Card>

      <Card className="animate-rise flex flex-col gap-3 py-4" style={rise(1)}>
        <div className="flex flex-wrap items-center gap-3">
          <label className="relative min-w-60 flex-1">
            <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("tweaks.search")}
              aria-label={t("tweaks.search")}
              className={cn(selectClass, "w-full pl-9")}
            />
          </label>
          <span className="text-xs text-muted-foreground tabular-nums">
            {t("tweaks.count", { on: active, total: present.length })}
          </span>
          <Button
            variant="primary"
            tactile
            disabled={!states || run !== null || pendingRunnable.length === 0}
            onClick={() => void applyRecommended()}
            title={pending.length === 0 ? t("tweaks.apply_recommended.none") : undefined}
          >
            {t("tweaks.apply_recommended")}
            {pendingRunnable.length > 0 ? (
              <span className="num rounded bg-black/20 px-1.5 text-xs">
                {pendingRunnable.length}
              </span>
            ) : null}
          </Button>
        </div>
        <fieldset className="flex flex-wrap gap-1.5">
          <legend className="sr-only">{t("tweaks.search")}</legend>
          {(["all", ...TWEAK_CATEGORIES] as const).map((c) => (
            <button
              key={c}
              type="button"
              aria-pressed={category === c}
              onClick={() => setCategory(c)}
              className={cn(
                "press rounded-full border px-3 py-1 text-xs font-semibold transition-colors duration-200 ease-out-expo",
                category === c
                  ? "border-border-strong bg-foreground/8 text-foreground"
                  : "border-border text-muted-foreground hover:text-foreground",
              )}
            >
              {c === "all" ? t("tweaks.filter.all") : t(categoryKey(c))}
              <span className="num ml-1.5 text-muted-foreground/70">
                {c === "all" ? TWEAKS.length : TWEAKS.filter((tw) => tw.category === c).length}
              </span>
            </button>
          ))}
        </fieldset>
      </Card>

      {!inTauri ? <p className="text-xs text-muted-foreground">{t("common.soon")}</p> : null}
      {shown.length === 0 ? (
        <Card>
          <EmptyState icon={<Search className="size-5" />} title={t("tweaks.empty")} />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
          {shown.map((tw, i) => (
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
              onToggle={(on, level) => void toggle({ tw, on, level })}
            />
          ))}
        </div>
      )}
    </>
  );
}

/** Detail codes the state badge already explains (everything else is shown, e.g. the plan name). */
const QUIET_DETAILS = new Set(["reboot_pending", "not_present", "needs_admin", "busy"]);

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
  const [changes, setChanges] = useState(false);
  const Icon = tw.icon;
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
      ) : state?.detail && !QUIET_DETAILS.has(state.detail) ? (
        <p className="text-[11px] text-muted-foreground">{state.detail}</p>
      ) : null}

      <div className="flex flex-col gap-1.5 text-[11px]">
        <details>
          <summary className="cursor-pointer text-muted-foreground underline decoration-dotted underline-offset-2 hover:text-foreground">
            {t("common.not_recommended")}
          </summary>
          <p className="mt-1 text-muted-foreground">{t(tweakWarn(tw.id))}</p>
        </details>
        <details onToggle={(e) => setChanges(e.currentTarget.open)}>
          <summary className="cursor-pointer text-muted-foreground underline decoration-dotted underline-offset-2 hover:text-foreground">
            {t("tweaks.changes")}
          </summary>
          {changes ? <TweakChanges id={tw.id} className="mt-1.5" /> : null}
        </details>
      </div>

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
            {unknownNeedsAdmin(state)
              ? t("boost.state.unknown_admin")
              : t(`boost.state.${state.status}` as const)}
          </Badge>
        ) : null}
        <Badge tone="neutral">{t(categoryKey(tw.category))}</Badge>
        {rec && !rec.recommended && rec.reason ? (
          <Badge
            tone="warning"
            title={t("boost.not_recommended_here", { reason: t(reasonKey(rec.reason)) })}
          >
            {t(reasonKey(rec.reason))}
          </Badge>
        ) : null}
        {tw.admin ? <Badge tone="neutral">{t("common.admin_required")}</Badge> : null}
        <RestartBadge restart={tw.restart} />
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
              {!elevated ? <ElevateButton route="boost" tab="ram" /> : null}
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
              <ElevateButton route="boost" tab="ram" />
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
        {!elevated ? <ElevateButton route="boost" tab="dns" /> : null}
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
        {!elevated && items?.some((i) => i.needs_admin) ? (
          <ElevateButton route="boost" tab="startup" />
        ) : null}
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
