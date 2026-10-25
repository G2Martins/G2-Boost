import { Gamepad2, Plus, RefreshCw, Search, Square, Zap } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { GameTweaksDialog, optionsOf } from "@/components/games/game-tweaks";
import { GamePoster } from "@/components/games/poster";
import { RoutePanel } from "@/components/games/route-panel";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { reducedMotion } from "@/components/ui/count-up";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  Skeleton,
  Switch,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import {
  backend,
  type GameEntry,
  type GameModeOptions,
  type GameModeSession,
  type GameTweaksInfo,
  inTauri,
  type Launcher,
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

/** Browser-only dev aid (`#/games/demo`): sample games so the e2e can drive the poster grid. */
const demo = (
  id: string,
  name: string,
  launcher: Launcher,
  enabled = false,
  running = false,
): GameEntry => ({
  id,
  name,
  launcher,
  install_dir: null,
  main_exe: launcher === "custom" ? null : `C:\\Jogos\\${name}\\jogo.exe`,
  size_bytes: launcher === "steam" ? 35e9 : null,
  enabled,
  running,
});
const DEMO = [
  demo("steam:10", "Arena Tática", "steam", true, true),
  demo("epic:Corrida", "Corrida Noturna", "epic"),
  demo("app:demo", "Reino Perdido", "riot", true),
  demo("exe:jogo.exe", "jogo", "custom"),
];

/** Lowercase without accents, so "acao" finds "Ação". */
const fold = (s: string) =>
  s
    .normalize("NFD")
    .replace(/\p{Diacritic}/gu, "")
    .toLowerCase();

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
  const errorText = useErrorText();
  const [games, setGames] = useState<GameEntry[] | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [session, setSession] = useState<GameModeSession | null>(null);
  const [elevated, setElevated] = useState(false);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [tweakId, setTweakId] = useState<string | null>(null);
  const [tweakOpen, setTweakOpen] = useState(false);
  const [tweakInfo, setTweakInfo] = useState<GameTweaksInfo | null>(null);
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Launcher | "all">("all");
  const [launching, setLaunching] = useState<string | null>(null);
  const [picking, setPicking] = useState(false);
  useEffect(() => {
    backend.gameTweaksInfo().then(setTweakInfo);
  }, []);
  // Dev aid: G2BOOST_START_TAB=route opens the first game's route (and traces it with autorun).
  useEffect(() => {
    if (boot.startTab === "route" && games?.[0] && selected === null) setSelected(games[0].id);
  }, [games, selected]);
  // The route opens below the library: bring it into view.
  useEffect(() => {
    if (!selected) return;
    const id = requestAnimationFrame(() =>
      document
        .getElementById("game-route")
        ?.scrollIntoView({ behavior: reducedMotion() ? "auto" : "smooth", block: "start" }),
    );
    return () => cancelAnimationFrame(id);
  }, [selected]);

  const load = useCallback(async (refresh = false) => {
    setBusy(refresh);
    const [list, s, sess, rs] = await Promise.all([
      backend.gamesList(refresh),
      backend.settingsGet(),
      backend.gameModeStatus(),
      backend.routineStatus(),
    ]);
    setGames(list ?? (!inTauri && boot.startTab === "demo" ? DEMO : []));
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

  const saved = (s: Settings) => {
    setSettings(s);
    void load();
  };

  const play = async (g: GameEntry) => {
    setLaunching(g.id);
    try {
      await backend.gameLaunch(g.id);
      toast(t("games.play.started", { game: g.name }), { tone: "accent" });
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      toast(
        ["no_exe", "shell_unavailable", "launch_failed", "unknown_game"].includes(m)
          ? t(`games.play.error.${m}` as I18nKey)
          : m === "app-only"
            ? t("common.app_only")
            : errorText(e),
        { tone: "danger", duration: 8000 },
      );
    } finally {
      setLaunching(null);
    }
  };

  const add = async () => {
    setPicking(true);
    try {
      const s = await backend.pickExe(null);
      if (!s) return; // cancelled in the Windows dialog
      const file = s.game_paths.at(-1)?.split("\\").pop() ?? "";
      toast(t("games.lib.added", { game: file.replace(/\.exe$/i, "") }), { tone: "success" });
      saved(s);
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      toast(m === "bad_exe" ? t("gametweaks.error.bad_exe") : errorText(e), { tone: "danger" });
    } finally {
      setPicking(false);
    }
  };

  const stop = async () => {
    await backend.gameModeStop();
    setSession(null);
    setNote(t("games.mode.stopped"));
  };

  const launchers = useMemo(() => {
    const n = new Map<Launcher, number>();
    for (const g of games ?? []) n.set(g.launcher, (n.get(g.launcher) ?? 0) + 1);
    return [...n.entries()];
  }, [games]);
  // A filter whose launcher left the list (after a refresh) shows everything again.
  const active = filter !== "all" && launchers.some(([l]) => l === filter) ? filter : "all";
  const shown = useMemo(() => {
    const q = fold(query.trim());
    return (games ?? []).filter(
      (g) => (active === "all" || g.launcher === active) && fold(g.name).includes(q),
    );
  }, [games, active, query]);
  const tweakGame = games?.find((g) => g.id === tweakId) ?? null;
  const selectedGame = games?.find((g) => g.id === selected);

  const chip = (on: boolean) =>
    cn(
      "press inline-flex h-8 items-center gap-1.5 rounded-full px-3 text-xs font-semibold transition-colors",
      on
        ? "bg-foreground/10 text-foreground ring-1 ring-inset ring-border-strong"
        : "text-muted-foreground hover:bg-foreground/5 hover:text-foreground",
    );

  return (
    <>
      <PageHeader
        title={t("games.title")}
        subtitle={t("games.subtitle")}
        actions={
          <div className="flex gap-2">
            <Button
              variant="outline"
              size="lg"
              disabled={picking || !settings || !inTauri}
              onClick={add}
            >
              <Plus className="size-4" />
              {t("games.lib.add")}
            </Button>
            <Button
              variant="outline"
              size="lg"
              disabled={busy || !inTauri}
              onClick={() => void load(true)}
            >
              <RefreshCw className={cn("size-4", busy && "animate-spin")} />
              {t("games.refresh")}
            </Button>
          </div>
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
                  {note ?? t("games.mode.hint")}
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

        {/* ---------------- library ---------------- */}
        <Card className="animate-rise" style={rise(1)}>
          <CardTitle help={t("games.lib.help")}>
            <Gamepad2 className="size-4" />
            {t("games.lib.title")}
            {games ? <Badge tone="neutral">{games.length}</Badge> : null}
          </CardTitle>
          {games && games.length > 0 ? (
            <div className="mb-4 flex flex-wrap items-center gap-3">
              <label className="relative flex min-w-56 flex-1 items-center sm:max-w-72">
                <Search
                  aria-hidden
                  className="pointer-events-none absolute left-2.5 size-4 text-muted-foreground"
                />
                <input
                  type="search"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  maxLength={60}
                  placeholder={t("games.lib.search")}
                  aria-label={t("games.lib.search")}
                  className="h-9 w-full rounded-md border border-border bg-background pr-3 pl-8.5 text-sm outline-none focus:border-ring"
                />
              </label>
              {launchers.length > 1 ? (
                <fieldset
                  aria-label={t("games.lib.filter")}
                  className="flex flex-wrap items-center gap-1"
                >
                  <button
                    type="button"
                    className={chip(active === "all")}
                    aria-pressed={active === "all"}
                    onClick={() => setFilter("all")}
                  >
                    {t("games.lib.all")}
                    <span className="num text-[10px] text-muted-foreground">{games.length}</span>
                  </button>
                  {launchers.map(([l, n]) => (
                    <button
                      key={l}
                      type="button"
                      className={chip(active === l)}
                      aria-pressed={active === l}
                      onClick={() => setFilter(l)}
                    >
                      {t(`games.launcher.${l}` as I18nKey)}
                      <span className="num text-[10px] text-muted-foreground">{n}</span>
                    </button>
                  ))}
                </fieldset>
              ) : null}
            </div>
          ) : null}

          {games === null ? (
            <ul className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4">
              {[0, 1, 2, 3, 4].map((k) => (
                <li key={k}>
                  <Skeleton className="aspect-[2/3] h-auto rounded-[calc(var(--radius)+2px)]" />
                </li>
              ))}
            </ul>
          ) : games.length === 0 ? (
            <EmptyState
              icon={<Gamepad2 className="size-5" />}
              title={t("games.lib.empty.title")}
              body={t("games.lib.empty.body")}
              action={
                <Button variant="outline" disabled={picking || !settings || !inTauri} onClick={add}>
                  <Plus className="size-4" />
                  {t("games.lib.add")}
                </Button>
              }
            />
          ) : shown.length === 0 ? (
            <p className="py-10 text-center text-sm text-muted-foreground">
              {t("games.lib.no_match")}
            </p>
          ) : (
            <ul className="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4">
              {shown.map((g, i) => (
                <GamePoster
                  key={g.id}
                  game={g}
                  style={rise(Math.min(i, 8))}
                  options={Object.values(optionsOf(settings, g.id)).filter(Boolean).length}
                  selected={selected === g.id}
                  launching={launching === g.id}
                  onPlay={() => void play(g)}
                  onTweaks={() => {
                    setTweakId(g.id);
                    setTweakOpen(true);
                  }}
                  onRoute={() => setSelected((cur) => (cur === g.id ? null : g.id))}
                />
              ))}
            </ul>
          )}
        </Card>

        <GameTweaksDialog
          game={tweakGame}
          open={tweakOpen}
          settings={settings}
          info={tweakInfo}
          onClose={() => setTweakOpen(false)}
          onSaved={saved}
        />

        {selectedGame ? (
          <div id="game-route" className="scroll-mt-4">
            <RoutePanel game={selectedGame} />
          </div>
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
