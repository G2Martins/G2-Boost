import { Eraser, ListTree, PowerOff, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { GameExesEditor } from "@/components/game-exes";
import { rise } from "@/components/shell/page-header";
import { CountUp } from "@/components/ui/count-up";
import { ExeIcon } from "@/components/ui/exe-icon";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  Modal,
  Skeleton,
  Switch,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import {
  type AppMem,
  backend,
  type GameEvent,
  inTauri,
  type MemComposition,
  type PurgeResult,
  type RoutineStatus,
  type Settings,
  type StartupItem,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { startupByExe } from "@/lib/ram-apps";
import { cn, formatBytes } from "@/lib/utils";

const POLL_MS = 2500;

/** The four parts of the bar, in Task Manager's order. Standby is hatched: cache, given back on demand. */
const SEGMENTS = [
  { key: "in_use", cls: "bg-chart-1" },
  { key: "modified", cls: "bg-chart-3" },
  {
    key: "standby",
    cls: "bg-[repeating-linear-gradient(135deg,hsl(var(--chart-2)/0.55)_0_5px,hsl(var(--chart-2)/0.28)_5px_10px)]",
  },
  { key: "free", cls: "bg-transparent" },
] as const;

/** RAM: what the memory is doing, the standby purge with its before/after, who holds memory. */
export function RamTab({
  elevated,
  onOpenProcs,
}: {
  elevated: boolean;
  onOpenProcs: (name: string) => void;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [comp, setComp] = useState<MemComposition | null>(null);
  const [apps, setApps] = useState<AppMem[] | null>(null);
  const [startup, setStartup] = useState<StartupItem[] | null>(null);
  const [last, setLast] = useState<{ at: Date; result: PurgeResult } | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [status, setStatus] = useState<RoutineStatus | null>(null);
  const [lastGame, setLastGame] = useState<GameEvent | null>(null);

  const refreshApps = useCallback(async () => {
    const a = await backend.ramApps();
    if (a) setApps(a);
  }, []);

  useEffect(() => {
    let alive = true;
    let n = 0;
    const tick = () => {
      if (document.hidden) return;
      backend.ramComposition().then((c) => alive && c && setComp(c));
      // The process walk is heavier than PDH: every other tick.
      if (n++ % 2 === 0) backend.ramApps().then((a) => alive && a && setApps(a));
      backend.routineStatus().then((s) => alive && s && setStatus(s));
    };
    tick();
    backend.startupList().then((s) => alive && s && setStartup(s));
    backend.settingsGet().then((s) => alive && s && setSettings(s));
    const id = window.setInterval(tick, POLL_MS);
    const off = backend.on<GameEvent>("game:started", setLastGame);
    return () => {
      alive = false;
      window.clearInterval(id);
      void off.then((u) => u());
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
      // Right away: the bar shows the cache turning into free memory.
      const c = await backend.ramComposition();
      if (c) setComp(c);
    } catch (e) {
      setErr(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const setStartupItem = async (it: StartupItem, on: boolean) => {
    try {
      const next = await backend.startupSet(it.location, it.name, on);
      setStartup((cur) =>
        (cur ?? []).map((x) => (x.location === it.location && x.name === it.name ? next : x)),
      );
      if (!on) {
        toast(t("ram.apps.startup_off_done", { name: it.name }), {
          tone: "success",
          action: { label: t("tweaks.toast.undo"), run: () => void setStartupItem(it, true) },
        });
      }
    } catch (e) {
      setErr(errorText(e));
    }
  };

  return (
    <>
      <Card className="animate-rise">
        <CardTitle
          help={t("boost.ram.help")}
          right={
            <span className="flex items-center gap-2">
              {!elevated ? <ElevateButton route="boost" tab="ram" /> : null}
              <Button
                variant="primary"
                tactile
                disabled={busy || !inTauri || !elevated}
                title={!elevated ? t("ram.purge.admin") : undefined}
                onClick={clean}
              >
                <Eraser className="size-4" />
                {t("boost.ram.clean")}
              </Button>
            </span>
          }
        >
          {t("ram.title")}
        </CardTitle>
        {err ? <p className="mb-3 text-xs text-destructive">{err}</p> : null}
        <Composition comp={comp} />
        {comp ? <Advice comp={comp} /> : null}
        {!inTauri ? (
          <p className="mt-3 text-xs text-muted-foreground">{t("common.app_only")}</p>
        ) : null}
        {last ? <PurgeOutcome at={last.at} r={last.result} /> : null}
        <p className="mt-4 text-[11px] text-muted-foreground">
          {last
            ? t("boost.ram.last", { when: last.at.toLocaleTimeString(locale) })
            : t("boost.ram.never")}
          {" · "}
          {t("boost.ram.notice")}
        </p>
      </Card>

      <Card className="animate-rise min-w-0" style={rise(1)}>
        <CardTitle help={t("ram.apps.help")}>{t("ram.apps.title")}</CardTitle>
        <RamApps
          apps={apps}
          startup={startup}
          elevated={elevated}
          onStartupOff={(it) => setStartupItem(it, false)}
          onOpenProcs={onOpenProcs}
          onEnded={refreshApps}
        />
      </Card>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card className="animate-rise" style={rise(2)}>
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
        </Card>
        <Card className="animate-rise" style={rise(3)}>
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

/** The segmented bar (widths follow the bytes, so a purge visibly drains the cache) + legend. */
function Composition({ comp }: { comp: MemComposition | null }) {
  const { t, locale } = useI18n();
  const total = comp?.total ?? 0;
  const label = comp
    ? SEGMENTS.map(
        (s) => `${t(`ram.seg.${s.key}` as I18nKey)} ${formatBytes(comp[s.key], locale)}`,
      ).join(", ")
    : t("ram.title");
  return (
    <div>
      {comp === null && inTauri ? (
        <Skeleton className="h-8 w-full rounded-md" />
      ) : (
        <div
          role="img"
          aria-label={label}
          className="flex h-8 w-full overflow-hidden rounded-md bg-foreground/6 ring-1 ring-inset ring-foreground/8"
        >
          {comp
            ? SEGMENTS.map((s) => (
                <span
                  key={s.key}
                  data-seg={s.key}
                  className={cn(
                    "h-full min-w-0 basis-0 border-r-2 border-card last:border-r-0 motion-safe:transition-[flex-grow] motion-safe:duration-[var(--motion-slow)] motion-safe:ease-out-expo",
                    s.cls,
                  )}
                  style={{ flexGrow: total ? comp[s.key] / total : 0 }}
                />
              ))
            : null}
        </div>
      )}
      <dl className="mt-4 grid grid-cols-2 gap-x-6 gap-y-3 lg:grid-cols-4">
        {SEGMENTS.map((s) => (
          <div key={s.key} className="min-w-0">
            <dt className="flex items-center gap-2 text-xs font-semibold">
              <span
                aria-hidden
                className={cn(
                  "size-2.5 shrink-0 rounded-sm ring-1 ring-inset ring-foreground/15",
                  s.cls,
                )}
              />
              {t(`ram.seg.${s.key}` as I18nKey)}
            </dt>
            <dd className="mt-1">
              <span className="num font-display text-[17px] font-semibold tracking-wide">
                {comp ? formatBytes(comp[s.key], locale) : "—"}
              </span>
              {comp && total ? (
                <span className="num ml-1.5 text-[11px] text-muted-foreground">
                  {Math.round((comp[s.key] / total) * 100)}%
                </span>
              ) : null}
              <span className="mt-0.5 block text-[11px] leading-snug text-muted-foreground">
                {t(`ram.seg.${s.key}.hint` as I18nKey)}
              </span>
            </dd>
          </div>
        ))}
      </dl>
      {comp ? (
        <p className="num mt-3 text-[11px] text-muted-foreground">
          {t("boost.ram.total", { total: formatBytes(comp.total, locale, 0) })}
          {comp.compressed
            ? ` · ${t("ram.compressed", { size: formatBytes(comp.compressed, locale) })}`
            : ""}
          {comp.commit_limit
            ? ` · ${t("ram.commit", {
                used: formatBytes(comp.committed, locale),
                limit: formatBytes(comp.commit_limit, locale, 0),
              })}`
            : ""}
        </p>
      ) : null}
    </div>
  );
}

/** When a purge helps and when it does not, from the live split (the rule lives in the core). */
function Advice({ comp }: { comp: MemComposition }) {
  const { t } = useI18n();
  return (
    <p
      className={cn(
        "mt-4 rounded-md px-3 py-2.5 text-xs leading-relaxed ring-1 ring-inset",
        comp.advice === "helps"
          ? "bg-primary/10 text-foreground ring-primary/25"
          : comp.advice === "in_use"
            ? "bg-warning/10 text-foreground ring-warning/25"
            : "bg-foreground/4 text-foreground/90 ring-foreground/10",
      )}
    >
      {t(`ram.advice.${comp.advice}` as I18nKey)}
    </p>
  );
}

function PurgeOutcome({ at, r }: { at: Date; r: PurgeResult }) {
  const { t, locale } = useI18n();
  const f = (n: number) => formatBytes(n, locale);
  return (
    <div className="mt-4 flex flex-wrap items-center gap-x-6 gap-y-2 border-t border-border pt-4">
      <div>
        <p className="text-[11px] font-semibold uppercase tracking-[0.08em] text-muted-foreground">
          {r.freed_bytes > 0 ? t("ram.purge.freed") : t("ram.purge.none")}
        </p>
        {r.freed_bytes > 0 ? (
          <CountUp
            key={at.getTime()}
            value={r.freed_bytes}
            format={f}
            className="num font-display text-[26px] font-semibold tracking-wide text-primary"
          />
        ) : null}
      </div>
      {r.standby_before || r.free_before ? (
        <ul className="num space-y-1 text-xs text-muted-foreground">
          <li>
            {t("ram.purge.standby", { before: f(r.standby_before), after: f(r.standby_after) })}
          </li>
          <li>{t("ram.purge.free", { before: f(r.free_before), after: f(r.free_after) })}</li>
        </ul>
      ) : null}
    </div>
  );
}

/**
 * Apps by private memory with their own icon. Actions: open them in the task manager, end them
 * (every process the core allows, after confirmation) and, when they start with Windows, turn
 * that off (Task Manager semantics, reversible).
 */
export function RamApps({
  apps,
  startup,
  elevated,
  limit = 14,
  onStartupOff,
  onOpenProcs,
  onEnded,
}: {
  apps: AppMem[] | null;
  startup: StartupItem[] | null;
  elevated: boolean;
  limit?: number;
  onStartupOff: (it: StartupItem) => void;
  onOpenProcs?: (name: string) => void;
  onEnded: () => void;
}) {
  const { t, locale } = useI18n();
  const [confirm, setConfirm] = useState<AppMem | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const byExe = useMemo(() => startupByExe(startup), [startup]);
  const rows = (apps ?? []).slice(0, limit);
  const max = Math.max(1, ...rows.map((a) => a.private_bytes));

  const end = async (a: AppMem) => {
    setConfirm(null);
    let ok = 0;
    for (const p of a.procs) {
      try {
        await backend.procsEnd(p.pid, p.created);
        ok++;
      } catch {
        // gone already, or refused by the core: counted below
      }
    }
    const failed = a.procs.length - ok;
    setMsg(
      t("ram.apps.ended", { name: a.name, n: ok }) +
        (failed ? ` ${t("ram.apps.end_failed", { n: failed })}` : ""),
    );
    onEnded();
  };

  return (
    <>
      {apps === null && inTauri
        ? [0, 1, 2, 3, 4].map((k) => <Skeleton key={k} className="my-2.5 h-9 w-full" />)
        : null}
      {apps !== null && rows.length === 0 ? (
        <p className="py-6 text-center text-xs text-muted-foreground">{t("ram.apps.empty")}</p>
      ) : null}
      {!inTauri ? (
        <p className="py-6 text-center text-xs text-muted-foreground">{t("common.app_only")}</p>
      ) : null}
      <ul className="divide-y divide-border/70">
        {rows.map((a) => {
          const item = byExe.get(a.path.toLowerCase());
          const starts = item?.enabled === true;
          const lockedStartup = item?.protected || (item?.needs_admin && !elevated);
          return (
            <li key={a.path} className="flex flex-wrap items-center gap-x-3 gap-y-1.5 py-2.5">
              <ExeIcon path={a.path} name={a.name} className="size-7" />
              <div className="min-w-40 flex-1">
                <p className="flex flex-wrap items-center gap-1.5">
                  <span className="truncate text-sm font-semibold" title={a.path}>
                    {a.name}
                  </span>
                  {a.count > 1 ? (
                    <span className="num text-[11px] text-muted-foreground">×{a.count}</span>
                  ) : null}
                  {a.system ? <Badge tone="neutral">{t("boost.procs.system")}</Badge> : null}
                  {starts ? <Badge tone="accent">{t("ram.apps.startup")}</Badge> : null}
                </p>
                <span className="mt-1 block h-1 overflow-hidden rounded-full bg-foreground/6">
                  <span
                    className={cn(
                      "block h-full rounded-full motion-safe:transition-[width] motion-safe:duration-[var(--motion-base)] motion-safe:ease-out-expo",
                      a.system ? "bg-muted-foreground/40" : "bg-chart-1",
                    )}
                    style={{ width: `${(a.private_bytes / max) * 100}%` }}
                  />
                </span>
              </div>
              <span className="w-28 text-right">
                <span className="num block text-sm font-semibold">
                  {formatBytes(a.private_bytes, locale)}
                </span>
                <span className="num block text-[10px] text-muted-foreground">
                  {t("ram.apps.ws", { size: formatBytes(a.ws_bytes, locale) })}
                </span>
              </span>
              <span className="flex items-center gap-1">
                {starts && item ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!!lockedStartup}
                    title={item.needs_admin && !elevated ? t("ram.apps.startup_admin") : undefined}
                    onClick={() => onStartupOff(item)}
                  >
                    <PowerOff className="size-3.5" />
                    {t("ram.apps.startup_off")}
                  </Button>
                ) : null}
                {onOpenProcs ? (
                  <button
                    type="button"
                    onClick={() => onOpenProcs(a.name)}
                    aria-label={t("ram.apps.open_named", { name: a.name })}
                    title={t("ram.apps.open")}
                    className="press flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/6 hover:text-foreground"
                  >
                    <ListTree className="size-4" />
                  </button>
                ) : null}
                <button
                  type="button"
                  disabled={a.procs.length === 0}
                  onClick={() => setConfirm(a)}
                  aria-label={t("procs.end_named", { name: a.name })}
                  title={
                    a.procs.length === 0
                      ? t(`procs.kind.${a.protected ?? "system"}` as I18nKey)
                      : t("procs.end")
                  }
                  className="press flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-destructive/12 hover:text-destructive disabled:pointer-events-none disabled:opacity-35"
                >
                  <X className="size-4" />
                </button>
              </span>
            </li>
          );
        })}
      </ul>
      {msg ? <p className="mt-2 text-xs text-foreground/90">{msg}</p> : null}
      <Modal
        open={confirm !== null}
        onClose={() => setConfirm(null)}
        closeLabel={t("common.cancel")}
        title={t("procs.confirm_title", { name: confirm?.name ?? "" })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={() => confirm && end(confirm)}>
              {t("procs.end")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">
          {t("ram.apps.end_body", { n: confirm?.procs.length ?? 0 })}
        </p>
        <p
          className="mt-2 truncate font-mono text-[11px] text-muted-foreground/80"
          title={confirm?.path}
        >
          {confirm?.path}
        </p>
      </Modal>
    </>
  );
}
