import { Loader2, Play, Route as RouteIcon, SlidersHorizontal, Zap } from "lucide-react";
import { type CSSProperties, useState } from "react";
import { Badge, Button } from "@/components/ui/primitives";
import { type GameEntry, inTauri } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

/** Cover or big exe icon, served by the app itself (`cover` URI scheme). None in the browser. */
const art = (id: string, icon = false) =>
  inTauri ? `http://cover.localhost/${icon ? "icon/" : ""}${encodeURIComponent(id)}` : null;

/** Short tag of the exe path: a new exe gets a new icon URL, never the old cached image. */
const tag = (s: string) => [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) | 0, 7).toString(36);

/** Steam and Epic start through their launcher; the rest need an exe. */
export const launchable = (g: GameEntry) =>
  g.launcher === "steam" || g.launcher === "epic" || g.main_exe !== null;

/** Legible over any cover: the chip's tone on a frosted dark base. */
const ON_ART = "bg-background/80 backdrop-blur-sm";

/**
 * One game as a 2:3 poster: cover (Steam) or a tile made of its own exe icon, name over the
 * scrim, launcher and status chips. Hover or keyboard focus reveals "Preparar e jogar" plus
 * the game's options and route.
 */
export function GamePoster({
  game,
  options,
  selected,
  launching,
  style,
  onPlay,
  onTweaks,
  onRoute,
}: {
  game: GameEntry;
  /** How many per-game options are on (shown on the Ajustes button). */
  options: number;
  selected: boolean;
  launching: boolean;
  style?: CSSProperties;
  onPlay: () => void;
  onTweaks: () => void;
  onRoute: () => void;
}) {
  const { t, locale } = useI18n();
  const cover = game.launcher === "steam" ? art(game.id) : null;
  const base = game.main_exe ? art(game.id, true) : null;
  const icon = base && game.main_exe ? `${base}?v=${tag(game.main_exe)}` : null;
  // The src that failed (not a flag): a new src gets its own try.
  const [failed, setFailed] = useState<string[]>([]);
  const fail = (src: string) => setFailed((f) => [...f, src]);
  const showCover = cover !== null && !failed.includes(cover);
  const showIcon = !showCover && icon !== null && !failed.includes(icon);
  const canPlay = launchable(game);
  return (
    <li
      style={style}
      className={cn(
        "poster group animate-rise outline-offset-2",
        selected ? "outline-2 outline-primary/80" : game.running && "outline-1 outline-primary/55",
      )}
    >
      {showCover ? (
        <img
          src={cover}
          alt=""
          loading="lazy"
          decoding="async"
          draggable={false}
          onError={() => fail(cover)}
        />
      ) : showIcon ? (
        // The tile's backdrop: the same icon, blown up and blurred into a wash of its colors.
        <img
          src={icon}
          alt=""
          loading="lazy"
          draggable={false}
          onError={() => fail(icon)}
          className="opacity-45 blur-2xl saturate-150"
          style={{ transform: "scale(1.7)" }}
        />
      ) : null}

      {showCover ? null : (
        <div
          aria-hidden
          className="absolute inset-0 flex items-center justify-center bg-[radial-gradient(70%_55%_at_50%_38%,hsl(var(--primary)/0.10),transparent_70%)] pb-14"
        >
          <span className="frame-ticks absolute inset-x-5 top-11 bottom-18" />
          {showIcon ? (
            <img
              src={icon}
              alt=""
              draggable={false}
              className="size-20 object-contain drop-shadow-[0_10px_18px_hsl(0_0%_0%/0.55)]"
            />
          ) : (
            <span className="font-display text-6xl font-semibold text-foreground/20">
              {game.name.slice(0, 1).toUpperCase()}
            </span>
          )}
        </div>
      )}

      {/* hover / focus veil: the art steps back, the actions come forward */}
      <div
        aria-hidden
        className="pointer-events-none absolute inset-0 bg-background/60 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 motion-safe:transition-opacity motion-safe:duration-300"
      />

      <div className="pointer-events-none absolute inset-x-2 top-2 flex items-start justify-between gap-1">
        <Badge tone="neutral" className={cn(ON_ART, "block max-w-[62%] truncate")}>
          {t(`games.launcher.${game.launcher}` as I18nKey)}
        </Badge>
        <span className="flex flex-col items-end gap-1">
          {game.running ? (
            <Badge tone="accent" className={cn(ON_ART, "gap-1")}>
              <span className="size-1.5 rounded-full bg-primary motion-safe:animate-pulse" />
              {t("games.playing")}
            </Badge>
          ) : null}
          {game.enabled ? (
            <Badge tone="accent" className={cn(ON_ART, "gap-1")}>
              <Zap className="size-3" />
              {t("games.mode.title")}
            </Badge>
          ) : null}
        </span>
      </div>

      <div className="pointer-events-none absolute inset-x-0 bottom-0 p-3">
        <h3
          title={game.name}
          className="line-clamp-2 font-display text-[15px] font-semibold leading-tight tracking-wide [text-shadow:0_1px_10px_hsl(var(--background)/0.9)]"
        >
          {game.name}
        </h3>
        {game.size_bytes ? (
          <p className="num mt-0.5 text-[10px] text-muted-foreground">
            {formatBytes(game.size_bytes, locale)}
          </p>
        ) : null}
      </div>

      <div className="absolute inset-0 flex flex-col justify-center gap-2 px-3 pb-12 opacity-0 group-focus-within:opacity-100 group-hover:opacity-100 motion-safe:translate-y-1.5 motion-safe:transition-[opacity,translate] motion-safe:duration-300 motion-safe:ease-out-expo motion-safe:group-focus-within:translate-y-0 motion-safe:group-hover:translate-y-0">
        <Button
          variant="primary"
          tactile
          className="w-full px-3 text-[12.5px]"
          disabled={!canPlay || launching}
          onClick={onPlay}
          aria-label={t("games.play.aria", { game: game.name })}
          title={canPlay ? undefined : t("games.play.error.no_exe")}
        >
          {launching ? (
            <Loader2 className="size-4 animate-spin" />
          ) : (
            <Play className="size-4 fill-current" />
          )}
          {t("games.play")}
        </Button>
        <div className="flex gap-2">
          <Button
            size="sm"
            className={cn(ON_ART, "flex-1 px-2 hover:bg-background")}
            onClick={onTweaks}
            aria-label={t("gametweaks.open.aria", { game: game.name })}
          >
            <SlidersHorizontal className="size-3.5" />
            {t("games.tweaks")}
            {options ? (
              <span className="num flex size-4 items-center justify-center rounded-full bg-primary text-[9px] font-bold text-primary-foreground">
                {options}
              </span>
            ) : null}
          </Button>
          <Button
            size="sm"
            className={cn(
              ON_ART,
              "flex-1 px-2 hover:bg-background",
              selected && "text-primary ring-1 ring-primary/40 ring-inset",
            )}
            onClick={onRoute}
            aria-pressed={selected}
            aria-label={t("route.title", { game: game.name })}
          >
            <RouteIcon className="size-3.5" />
            {t("games.route")}
          </Button>
        </div>
      </div>
    </li>
  );
}
