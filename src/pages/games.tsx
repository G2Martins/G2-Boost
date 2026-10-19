import { Gamepad2, Plus, RefreshCw, Square, Zap } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ElevateButton } from "@/components/elevate-button";
import { RoutePanel } from "@/components/games/route-panel";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, Skeleton, Switch } from "@/components/ui/primitives";
import {
  backend,
  type GameEntry,
  type GameModeOptions,
  type GameModeSession,
  inTauri,
  type Settings,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

const OPTIONS: (keyof GameModeOptions)[] = [
  "power_plan",
  "high_priority",
  "purge_ram",
  "network_priority",
  "pause_routines",
];

/** A game's icon (its main executable's); its initial on a tinted tile until it loads. */
function GameIcon({ game }: { game: GameEntry }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    backend.gamesIcon(game.id).then((s) => alive && setSrc(s));
    return () => {
      alive = false;
    };
  }, [game.id]);
  return src ? (
    <img src={src} alt="" className="size-11 shrink-0 rounded-lg object-contain" />
  ) : (
    <span
      aria-hidden
      className="flex size-11 shrink-0 items-center justify-center rounded-lg bg-primary/12 font-display text-lg font-semibold text-primary"
    >
      {game.name.slice(0, 1).toUpperCase()}
    </span>
  );
}

function Minutes({ since }: { since: number }) {
  const { t } = useI18n();
  const [now, setNow] = useState(() => Date.now() / 1000);
  useEffect(() => {
    const id = setInterval(() => setNow(Date.now() / 1000), 30_000);
    return () => clearInterval(id);
  }, []);
  return <>{t("games.mode.since", { min: Math.max(0, Math.floor((now - since) / 60)) })}</>;
}

export function GamesPage() {
  const { t, locale } = useI18n();
  const [games, setGames] = useState<GameEntry[] | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [session, setSession] = useState<GameModeSession | null>(null);
  const [elevated, setElevated] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [exe, setExe] = useState("");
  const [exeError, setExeError] = useState(false);
  const [selected, setSelected] = useState<string | null>(null);
  // Dev aid: G2BOOST_START_TAB=route opens the first game's route (and traces it with autorun).
  useEffect(() => {
    if (boot.startTab === "route" && games?.[0] && selected === null) setSelected(games[0].id);
  }, [games, selected]);

  const load = useCallback(async (refresh = false) => {
    setBusy(refresh);
    const [list, s, sess, rs] = await Promise.all([
      backend.gamesList(refresh),
      backend.settingsGet(),
      backend.gameModeStatus(),
      backend.routineStatus(),
    ]);
    setGames(list ?? []);
    if (s) setSettings(s);
    setSession(sess);
    setElevated(!!rs?.elevated);
    setBusy(false);
  }, []);

  useEffect(() => {
    void load();
    // "Playing" flags change without an event for games without Game Mode: refresh now and then.
    const id = setInterval(() => void load(), 15_000);
    const off = backend.on<GameModeSession | null>("gamemode:changed", (s) => {
      setSession(s);
      void load();
    });
    return () => {
      clearInterval(id);
      off.then((f) => f());
    };
  }, [load]);

  const save = async (next: Settings) => {
    setSettings(next);
    try {
      setSettings(await backend.settingsSet(next));
    } catch {
      void load();
    }
  };

  const toggleGame = (g: GameEntry, on: boolean) => {
    if (!settings) return;
    const ids = new Set(settings.game_mode_games);
    if (on) ids.add(g.id);
    else ids.delete(g.id);
    setGames((cur) => cur?.map((x) => (x.id === g.id ? { ...x, enabled: on } : x)) ?? cur);
    void save({ ...settings, game_mode_games: [...ids] });
  };

  const addExe = async () => {
    const v = exe.trim().toLowerCase();
    if (!/^[^\\/:*?"<>|]{1,60}\.exe$/.test(v) || !settings) {
      setExeError(true);
      return;
    }
    await save({ ...settings, game_exes: [...new Set([...settings.game_exes, v])] });
    setExe("");
    setAdding(false);
    setExeError(false);
    void load(true);
  };

  const stop = async () => {
    await backend.gameModeStop();
    setSession(null);
    setNote(t("games.mode.stopped"));
  };

  return (
    <>
      <PageHeader
        title={t("games.title")}
        subtitle={t("games.subtitle")}
        actions={
          <Button
            variant="outline"
            size="lg"
            disabled={busy || !inTauri}
            onClick={() => void load(true)}
          >
            <RefreshCw className={cn("size-4", busy && "animate-spin")} />
            {t("games.refresh")}
          </Button>
        }
      />
      <PageBody>
        {/* ---------------- Game Mode status ---------------- */}
        <Card
          className={cn(
            "animate-rise relative overflow-hidden",
            session && "border-primary/50 shadow-[inset_0_0_0_1px_hsl(var(--primary)/0.25)]",
          )}
        >
          {session ? (
            <span
              aria-hidden
              className="pointer-events-none absolute -top-24 -right-16 size-64 rounded-full bg-primary/15 blur-3xl"
            />
          ) : null}
          <div className="relative flex flex-wrap items-center gap-4">
            <span
              className={cn(
                "flex size-12 items-center justify-center rounded-xl",
                session
                  ? "bg-primary text-primary-foreground"
                  : "bg-foreground/6 text-muted-foreground",
              )}
            >
              <Zap className="size-6" />
            </span>
            <div className="min-w-0 flex-1">
              <p className="font-display text-lg font-semibold tracking-wide">
                {session
                  ? t("games.mode.active", { game: session.game_name })
                  : t("games.mode.title")}
              </p>
              {session ? (
                <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs">
                  <span className="text-muted-foreground">
                    <Minutes since={session.started} />
                  </span>
                  {session.applied.map((a) => (
                    <Badge key={a} tone="success">
                      {a === "ram"
                        ? t("games.applied.ram", { size: formatBytes(session.ram_freed, locale) })
                        : t(`games.applied.${a}` as I18nKey)}
                    </Badge>
                  ))}
                  {session.skipped.map((k) => (
                    <Badge key={k} tone="neutral">
                      {t(`games.skipped.${k}` as I18nKey)}
                    </Badge>
                  ))}
                </div>
              ) : (
                <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
                  {note ?? t("games.mode.idle")}
                </p>
              )}
            </div>
            {session ? (
              <Button variant="outline" onClick={stop}>
                <Square className="size-4" />
                {t("games.mode.stop")}
              </Button>
            ) : null}
          </div>
        </Card>

        {/* ---------------- games grid ---------------- */}
        <Card className="animate-rise" style={rise(1)}>
          <CardTitle help={t("games.list.help")}>
            <Gamepad2 className="size-4" />
            {t("games.list.title")}
            {games ? <Badge tone="neutral">{games.length}</Badge> : null}
          </CardTitle>
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4">
            {games === null
              ? [0, 1, 2, 3].map((k) => <Skeleton key={k} className="h-20 w-full" />)
              : games.map((g, i) => (
                  <div
                    key={g.id}
                    style={rise(Math.min(i, 8))}
                    className={cn(
                      "animate-rise group relative flex items-center gap-3 rounded-lg border bg-background/50 p-3 transition-colors",
                      g.running
                        ? "border-primary/60 bg-primary/5"
                        : g.enabled
                          ? "border-border-strong"
                          : "border-border hover:border-border-strong",
                      selected === g.id && "ring-1 ring-primary/50",
                    )}
                  >
                    <GameIcon game={g} />
                    <button
                      type="button"
                      onClick={() => setSelected((cur) => (cur === g.id ? null : g.id))}
                      aria-expanded={selected === g.id}
                      title={t("route.open")}
                      className="min-w-0 flex-1 text-left"
                    >
                      <p className="truncate text-sm font-semibold" title={g.name}>
                        {g.name}
                      </p>
                      <p className="flex items-center gap-1.5 truncate text-[11px] text-muted-foreground">
                        {t(`games.launcher.${g.launcher}` as I18nKey)}
                        {g.running ? (
                          <span className="flex items-center gap-1 font-semibold text-primary">
                            <span className="size-1.5 rounded-full bg-primary animate-pulse" />
                            {t("games.playing")}
                          </span>
                        ) : null}
                      </p>
                    </button>
                    <Switch
                      checked={g.enabled}
                      onCheckedChange={(on) => toggleGame(g, on)}
                      disabled={!settings}
                      aria-label={t("games.mode.toggle", { game: g.name })}
                    />
                  </div>
                ))}
            {/* add an executable by hand */}
            <div className="flex min-h-20 items-center rounded-lg border border-dashed border-border-strong p-3">
              {adding ? (
                <form
                  className="flex w-full flex-col gap-1.5"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void addExe();
                  }}
                >
                  <div className="flex gap-2">
                    <input
                      // biome-ignore lint/a11y/noAutofocus: opened by the user's click
                      autoFocus
                      value={exe}
                      onChange={(e) => {
                        setExe(e.target.value);
                        setExeError(false);
                      }}
                      maxLength={64}
                      placeholder={t("games.add.placeholder")}
                      aria-label={t("games.add.title")}
                      className="h-8 min-w-0 flex-1 rounded-md border border-border bg-background px-2.5 font-mono text-xs outline-none focus:border-ring"
                    />
                    <Button size="sm" variant="outline" type="submit">
                      {t("games.add.save")}
                    </Button>
                  </div>
                  {exeError ? (
                    <span className="text-[11px] text-destructive">{t("games.add.invalid")}</span>
                  ) : null}
                </form>
              ) : (
                <button
                  type="button"
                  onClick={() => setAdding(true)}
                  disabled={!settings}
                  className="press flex w-full items-center justify-center gap-2 text-sm text-muted-foreground hover:text-foreground disabled:opacity-40"
                >
                  <Plus className="size-4" />
                  {t("games.add.title")}
                </button>
              )}
            </div>
          </div>
          {games && games.length === 0 ? (
            <p className="mt-3 text-xs text-muted-foreground">{t("games.list.empty")}</p>
          ) : null}
        </Card>

        {selected && games?.find((g) => g.id === selected) ? (
          <RoutePanel game={games.find((g) => g.id === selected) as GameEntry} />
        ) : null}

        {/* ---------------- what Game Mode does ---------------- */}
        <Card className="animate-rise" style={rise(2)}>
          <CardTitle help={t("games.options.help")}>{t("games.options.title")}</CardTitle>
          <ul className="divide-y divide-border">
            {OPTIONS.map((k) => (
              <li key={k} className="flex items-center gap-4 py-3">
                <div className="min-w-0 flex-1">
                  <p className="flex flex-wrap items-center gap-2 text-sm font-semibold">
                    {t(`games.options.${k}` as I18nKey)}
                    {(k === "purge_ram" || k === "network_priority") && !elevated ? (
                      <ElevateButton route="games" />
                    ) : null}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {t(`games.options.${k}.desc` as I18nKey)}
                  </p>
                </div>
                <Switch
                  checked={settings?.game_mode[k] ?? false}
                  disabled={!settings}
                  onCheckedChange={(on) =>
                    settings &&
                    void save({ ...settings, game_mode: { ...settings.game_mode, [k]: on } })
                  }
                  aria-label={t(`games.options.${k}` as I18nKey)}
                />
              </li>
            ))}
          </ul>
          <p className="mt-3 text-[11px] leading-relaxed text-muted-foreground">
            {t("games.options.notifications")}
          </p>
        </Card>
      </PageBody>
    </>
  );
}
