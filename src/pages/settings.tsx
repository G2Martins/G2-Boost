import { CalendarClock, Gamepad2, Power, ShieldCheck } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { GameExesEditor } from "@/components/game-exes";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Badge, Card, CardTitle, MetricRow, Switch, selectClass } from "@/components/ui/primitives";
import { CLEANUP_CATALOG, cleanupName } from "@/data/catalogs";
import {
  type AppInfo,
  backend,
  inTauri,
  type RoutineStatus,
  type Schedule,
  type Settings,
} from "@/lib/backend";
import { type Locale, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const MODES = ["off", "at_logon", "every_hours", "daily", "weekly"] as const;
const HOURS = Array.from({ length: 24 }, (_, h) => h);
type Mode = (typeof MODES)[number];

function scheduleFor(mode: Mode, prev: Schedule): Schedule {
  const hour = "hour" in prev ? prev.hour : 9;
  switch (mode) {
    case "off":
      return { mode };
    case "at_logon":
      return { mode };
    case "every_hours":
      return { mode, hours: prev.mode === "every_hours" ? prev.hours : 12 };
    case "daily":
      return { mode, hour };
    case "weekly":
      return { mode, weekday: prev.mode === "weekly" ? prev.weekday : 0, hour };
  }
}

export function SettingsPage() {
  const { t, locale, setLocale } = useI18n();
  const errorText = useErrorText();
  const [info, setInfo] = useState<AppInfo | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [autostart, setAutostart] = useState<boolean | null>(null);
  const [status, setStatus] = useState<RoutineStatus | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    backend.appInfo().then((i) => alive && setInfo(i));
    backend.settingsGet().then((s) => alive && s && setSettings(s));
    backend.autostartStatus().then((a) => alive && a && setAutostart(a.enabled));
    const tick = () => backend.routineStatus().then((s) => alive && s && setStatus(s));
    tick();
    const id = setInterval(tick, 5000);
    return () => {
      alive = false;
      clearInterval(id);
    };
  }, []);

  const save = useCallback(
    async (patch: Partial<Settings>) => {
      if (!settings) return;
      setErr(null);
      try {
        setSettings(await backend.settingsSet({ ...settings, ...patch }));
      } catch (e) {
        setErr(errorText(e));
      }
    },
    [settings, errorText],
  );

  const toggleAutostart = async (on: boolean) => {
    setErr(null);
    try {
      setAutostart((await backend.autostartSet(on)).enabled);
    } catch (e) {
      setErr(errorText(e));
    }
  };

  const elevated = info?.elevated ?? false;
  const sched = settings?.schedule ?? { mode: "off" as const };

  return (
    <>
      <PageHeader title={t("settings.title")} subtitle={t("settings.subtitle")} />
      <PageBody>
        {err ? <p className="text-xs text-destructive">{err}</p> : null}

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
          <Card className="animate-rise">
            <CardTitle
              help={t("settings.autostart.hint")}
              right={
                <Switch
                  checked={!!autostart}
                  disabled={autostart === null || !inTauri}
                  onCheckedChange={toggleAutostart}
                  aria-label={t("settings.autostart")}
                />
              }
            >
              <Power className="size-4" />
              {t("settings.autostart")}
            </CardTitle>
            <p className="text-xs text-muted-foreground">
              {elevated ? t("settings.autostart.highest") : t("settings.autostart.lua")}
            </p>
            {!elevated ? (
              <div className="mt-3">
                <ElevateButton route="settings" />
              </div>
            ) : null}
          </Card>

          <Card className="animate-rise" style={rise(1)}>
            <CardTitle
              help={t("settings.tray.hint")}
              right={
                <Switch
                  checked={!!settings?.close_to_tray}
                  disabled={!settings}
                  onCheckedChange={(v) => save({ close_to_tray: v })}
                  aria-label={t("settings.tray")}
                />
              }
            >
              {t("settings.tray")}
            </CardTitle>
            <p className="text-xs text-muted-foreground">{t("settings.tray.body")}</p>
          </Card>
          <Card className="animate-rise" style={rise(2)}>
            <CardTitle
              help={t("settings.restore_before_preset.hint")}
              right={
                <Switch
                  checked={!!settings?.restore_before_preset}
                  disabled={!settings}
                  onCheckedChange={(v) => save({ restore_before_preset: v })}
                  aria-label={t("settings.restore_before_preset")}
                />
              }
            >
              {t("settings.restore_before_preset")}
            </CardTitle>
            <p className="text-xs text-muted-foreground">
              {t("settings.restore_before_preset.body")}
            </p>
          </Card>
        </div>

        <Card className="animate-rise" style={rise(2)}>
          <CardTitle help={t("settings.routine.hint")}>
            <CalendarClock className="size-4" />
            {t("settings.routine")}
            <span className="ml-2 font-sans text-xs font-normal text-muted-foreground">
              {status?.running
                ? t("cleanup.running", { done: "…", total: "…" })
                : status?.last_run
                  ? t("settings.routine.last_run", {
                      when: new Date(status.last_run * 1000).toLocaleString(locale),
                    })
                  : t("settings.routine.never")}
            </span>
          </CardTitle>
          <div className="flex flex-wrap items-center gap-2">
            {MODES.map((m) => (
              <button
                key={m}
                type="button"
                disabled={!settings}
                onClick={() => save({ schedule: scheduleFor(m, sched) })}
                className={cn(
                  "rounded-md border px-3 py-2 font-display text-[12px] font-semibold tracking-wide transition-colors disabled:opacity-50",
                  sched.mode === m
                    ? "border-ring bg-card-hover text-foreground"
                    : "border-border bg-background text-muted-foreground hover:text-foreground",
                )}
              >
                {t(`settings.routine.mode.${m}` as const)}
              </button>
            ))}
            {sched.mode === "every_hours" ? (
              <label className="ml-2 flex items-center gap-2 text-xs text-muted-foreground">
                {t("settings.routine.hours")}
                <input
                  type="number"
                  min={1}
                  max={168}
                  value={sched.hours}
                  onChange={(e) =>
                    save({
                      schedule: {
                        mode: "every_hours",
                        hours: Math.min(168, Math.max(1, Number(e.target.value) || 1)),
                      },
                    })
                  }
                  className={cn(selectClass, "w-20")}
                />
              </label>
            ) : null}
            {sched.mode === "daily" || sched.mode === "weekly" ? (
              <label className="ml-2 flex items-center gap-2 text-xs text-muted-foreground">
                {t("settings.routine.hour")}
                <select
                  value={sched.hour}
                  onChange={(e) =>
                    save({ schedule: { ...sched, hour: Number(e.target.value) } as Schedule })
                  }
                  className={selectClass}
                >
                  {HOURS.map((h) => (
                    <option key={h} value={h}>
                      {String(h).padStart(2, "0")}:00
                    </option>
                  ))}
                </select>
              </label>
            ) : null}
            {sched.mode === "weekly" ? (
              <label className="flex items-center gap-2 text-xs text-muted-foreground">
                {t("settings.routine.weekday")}
                <select
                  value={sched.weekday}
                  onChange={(e) =>
                    save({ schedule: { ...sched, weekday: Number(e.target.value) } })
                  }
                  className={selectClass}
                >
                  {([0, 1, 2, 3, 4, 5, 6] as const).map((d) => (
                    <option key={d} value={d}>
                      {t(`settings.weekday.${d}` as const)}
                    </option>
                  ))}
                </select>
              </label>
            ) : null}
          </div>
          <p className="mt-4 mb-2 text-xs font-semibold text-muted-foreground">
            {t("settings.routine.categories")}
          </p>
          <ul className="grid grid-cols-1 gap-2 md:grid-cols-2 xl:grid-cols-3">
            {CLEANUP_CATALOG.map((c) => {
              const on = settings?.routine_categories.includes(c.id) ?? false;
              return (
                <li key={c.id} className="flex items-center gap-3 text-sm">
                  <Switch
                    checked={on}
                    disabled={!settings}
                    onCheckedChange={(v) =>
                      save({
                        routine_categories: v
                          ? [...(settings?.routine_categories ?? []), c.id]
                          : (settings?.routine_categories ?? []).filter((x) => x !== c.id),
                      })
                    }
                    aria-label={t(cleanupName(c.id))}
                  />
                  <span className={cn(!on && "text-muted-foreground")}>{t(cleanupName(c.id))}</span>
                  {c.admin ? (
                    <Badge className="bg-card-hover text-muted-foreground">
                      {t("common.admin_required")}
                    </Badge>
                  ) : null}
                  {c.risk === "high" ? (
                    <Badge className="bg-destructive/12 text-destructive">
                      {t("cleanup.risk.high")}
                    </Badge>
                  ) : null}
                </li>
              );
            })}
          </ul>
        </Card>

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <Card className="animate-rise" style={rise(3)}>
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
              <Gamepad2 className="size-4" />
              {t("settings.game.ram")}
            </CardTitle>
            <p className="text-xs text-muted-foreground">
              {status?.game
                ? t("settings.game.current", { name: status.game.name })
                : t("settings.game.none")}
            </p>
            {!elevated ? (
              <p className="mt-3 flex flex-wrap items-center gap-2 text-xs text-warning">
                {t("settings.game.needs_admin")}
                <ElevateButton route="settings" />
              </p>
            ) : (
              <p className="mt-3 flex items-center gap-2 text-xs text-success">
                <ShieldCheck className="size-4" />
                {t("boost.elevated")}
              </p>
            )}
          </Card>
          <Card className="animate-rise" style={rise(4)}>
            <CardTitle help={t("settings.game.exes.hint")}>{t("settings.game.exes")}</CardTitle>
            <GameExesEditor
              exes={settings?.game_exes ?? []}
              disabled={!settings}
              onChange={(game_exes) => save({ game_exes })}
            />
          </Card>
        </div>

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <Card className="animate-rise" style={rise(5)}>
            <CardTitle>{t("settings.language")}</CardTitle>
            <div className="flex gap-2">
              {(["pt-BR", "en"] as Locale[]).map((l) => (
                <button
                  key={l}
                  type="button"
                  onClick={() => setLocale(l)}
                  className={cn(
                    "rounded-md border px-4 py-2.5 font-display text-[13px] font-semibold",
                    locale === l
                      ? "border-ring bg-card-hover"
                      : "border-border bg-background text-muted-foreground hover:text-foreground",
                  )}
                >
                  {t(`settings.language.${l}` as const)}
                </button>
              ))}
            </div>
          </Card>
          <Card className="animate-rise" style={rise(6)}>
            <CardTitle>{t("settings.about")}</CardTitle>
            <div className="divide-y divide-border">
              <MetricRow label={t("settings.version")} value={info?.version ?? "—"} />
              <MetricRow label={t("settings.core")} value={info?.core_version ?? "—"} />
              <MetricRow label={t("settings.os")} value={info?.os ?? "—"} />
              <MetricRow
                label={t("settings.elevated")}
                value={info ? (info.elevated ? t("settings.yes") : t("settings.no")) : "—"}
                ok={info?.elevated}
              />
            </div>
            <p className="mt-4 text-xs text-muted-foreground">{t("settings.privacy")}</p>
          </Card>
        </div>
      </PageBody>
    </>
  );
}
