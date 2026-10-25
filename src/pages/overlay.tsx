import {
  ArrowDownLeft,
  ArrowDownRight,
  ArrowUpLeft,
  ArrowUpRight,
  Check,
  Eye,
  EyeOff,
  Gauge,
  List,
  ListChecks,
  MonitorPlay,
  MousePointerClick,
  Plus,
  RectangleHorizontal,
  ShieldCheck,
} from "lucide-react";
import { type ReactNode, useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, Switch, selectClass } from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import {
  backend,
  inTauri,
  type OverlayCorner,
  type OverlayFrame,
  type OverlayMetric,
  type OverlaySettings,
  type OverlayStatus,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { HUD_GROUPS, Hud, OVERLAY_DEFAULTS } from "@/overlay/hud";

const MODS = ["Ctrl", "Alt", "Shift"] as const;
/** The core's safe list (`overlay::KEYS`); the core validates again. */
const KEYS = [
  "F1",
  "F2",
  "F3",
  "F4",
  "F5",
  "F6",
  "F7",
  "F8",
  "F9",
  "F10",
  "F11",
  "Insert",
  "Home",
  "End",
  "PageUp",
  "PageDown",
  "Pause",
  "ScrollLock",
] as const;
/** Key cap names as printed on keyboards. */
const KEY_CAP: Record<string, string> = {
  PageUp: "Page Up",
  PageDown: "Page Down",
  ScrollLock: "Scroll Lock",
};

const CORNERS: { id: OverlayCorner; place: string; origin: string; Icon: typeof ArrowUpLeft }[] = [
  { id: "top_left", place: "left-4 top-4", origin: "top left", Icon: ArrowUpLeft },
  { id: "top_right", place: "right-4 top-4", origin: "top right", Icon: ArrowUpRight },
  { id: "bottom_left", place: "bottom-4 left-4", origin: "bottom left", Icon: ArrowDownLeft },
  { id: "bottom_right", place: "right-4 bottom-4", origin: "bottom right", Icon: ArrowDownRight },
];

const METRIC_GROUPS: { label: I18nKey; metrics: OverlayMetric[]; note?: I18nKey }[] = [
  { label: "overlay.group.perf", metrics: ["fps", "frame_time", "low1"] },
  {
    label: "overlay.group.cpu",
    metrics: ["cpu_pct", "cpu_mhz", "cpu_temp"],
    note: "overlay.note.cpu_temp",
  },
  {
    label: "overlay.group.gpu",
    metrics: ["gpu_pct", "gpu_temp", "gpu_mhz"],
    note: "overlay.note.gpu",
  },
  { label: "overlay.group.memory", metrics: ["ram", "vram"] },
  { label: "overlay.group.game", metrics: ["game"] },
];
const ORDER = HUD_GROUPS.flatMap((g) => g.metrics);

const GB = 1024 ** 3;
/** Example numbers for the preview (labelled as such on screen). */
const SAMPLE: Omit<OverlayFrame, "config" | "game"> = {
  fps: { fps: 144, frame_ms: 6.9, low1_fps: 112, p99_ms: 9.8, frames: 1440 },
  fps_state: "ok",
  sys: {
    seq: 0,
    ts: 0,
    cpu_pct: 34,
    cpu_mhz: 4620,
    cpu_power_w: null,
    cpu_temp_c: 61,
    gpu_pct: 97,
    gpu_mem_bytes: 6.2 * GB,
    gpu_temp_c: 71,
    gpu_core_mhz: 2505,
    gpu_mem_mhz: null,
    gpu_power_w: null,
    ram_used: 12.4 * GB,
    ram_total: 32 * GB,
    disk_pct: 0,
    disk_bytes_s: 0,
    net_rx_bytes_s: 0,
    net_tx_bytes_s: 0,
  },
};

function splitHotkey(h: string): { mods: string[]; key: string } {
  const parts = h.split("+");
  const key = parts.pop() ?? "F7";
  return { mods: parts, key };
}

/** Segmented / chip toggle (aria-pressed), like the other pages' filters. */
function Chip({
  pressed,
  onClick,
  disabled,
  children,
}: {
  pressed: boolean;
  onClick: () => void;
  disabled?: boolean;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "press flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-xs font-semibold transition-colors disabled:opacity-40",
        pressed
          ? "border-border-strong bg-foreground/8 text-foreground"
          : "border-border text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

function Field({ label, value, children }: { label: string; value?: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-2 border-t border-border pt-4">
      <div className="flex items-baseline justify-between gap-3">
        <span className="text-[13px] font-semibold">{label}</span>
        {value ? <span className="num text-xs text-muted-foreground">{value}</span> : null}
      </div>
      {children}
    </div>
  );
}

/** A stand-in game frame: dusk sky, ember glow on the horizon, two ridges. Each corner is a button. */
function Stage({
  frame,
  corner,
  onCorner,
  disabled,
}: {
  frame: OverlayFrame;
  corner: OverlayCorner;
  onCorner: (c: OverlayCorner) => void;
  disabled: boolean;
}) {
  const { t } = useI18n();
  // The HUD is real size; on a small preview it shrinks (from its corner) until the opposite
  // corner's button stays free, so all four corners can always be clicked.
  const stage = useRef<HTMLDivElement>(null);
  const hud = useRef<HTMLSpanElement>(null);
  const [fit, setFit] = useState(1);
  // biome-ignore lint/correctness/useExhaustiveDependencies: the HUD node is remounted per corner
  useLayoutEffect(() => {
    const s = stage.current;
    const h = hud.current;
    if (!s || !h) return;
    const measure = () => {
      const k = Math.min(
        1,
        (s.clientWidth - 120) / h.offsetWidth,
        (s.clientHeight - 40) / h.offsetHeight,
      );
      setFit(Number.isFinite(k) && k > 0 ? k : 1);
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(s);
    ro.observe(h);
    return () => ro.disconnect();
  }, [corner]);
  return (
    <div
      ref={stage}
      className="relative aspect-video w-full overflow-hidden rounded-lg border border-border-strong bg-background"
    >
      <div
        aria-hidden
        className="absolute inset-0 bg-[linear-gradient(180deg,hsl(var(--surface)),hsl(var(--background))_72%)]"
      />
      <div
        aria-hidden
        className="absolute inset-x-0 bottom-0 h-3/5 bg-[radial-gradient(55%_75%_at_64%_100%,hsl(var(--primary)/0.3),transparent_70%)]"
      />
      <div
        aria-hidden
        className="absolute inset-x-0 bottom-0 h-2/5 bg-foreground/5 [clip-path:polygon(0_62%,13%_40%,27%_58%,41%_20%,55%_50%,68%_28%,82%_55%,100%_34%,100%_100%,0_100%)]"
      />
      <div
        aria-hidden
        className="absolute inset-x-0 bottom-0 h-1/4 bg-black/45 [clip-path:polygon(0_46%,18%_72%,33%_36%,50%_64%,66%_40%,84%_72%,100%_50%,100%_100%,0_100%)]"
      />
      <div aria-hidden className="frame-ticks absolute inset-2.5" />
      {CORNERS.map(({ id, place, origin, Icon }) => {
        const active = id === corner;
        return (
          <button
            key={id}
            type="button"
            aria-pressed={active}
            aria-label={t(`overlay.corner.${id}` as I18nKey)}
            disabled={disabled}
            onClick={() => onCorner(id)}
            className={cn(
              "absolute flex rounded-md",
              place,
              // Already there: clicks fall through to the corner buttons under the HUD's box.
              active
                ? "pointer-events-none"
                : "press size-10 items-center justify-center border border-dashed border-foreground/20 text-muted-foreground/70 transition-colors hover:border-primary/60 hover:text-foreground",
            )}
          >
            {active ? (
              <span
                ref={hud}
                key={id}
                className="motion-safe:animate-[fade-in_200ms_var(--ease-out-expo)]"
                style={{ transform: `scale(${fit})`, transformOrigin: origin }}
              >
                <Hud frame={frame} />
              </span>
            ) : (
              <Icon className="size-4" />
            )}
          </button>
        );
      })}
    </div>
  );
}

export function OverlayPage() {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [cfg, setCfg] = useState<OverlaySettings>(OVERLAY_DEFAULTS);
  const [status, setStatus] = useState<OverlayStatus | null>(null);
  const [live, setLive] = useState<OverlayFrame | null>(null);
  const [loaded, setLoaded] = useState(!inTauri);
  const [busy, setBusy] = useState(false);
  // Sliders and chips save 250 ms after the last change; leaving the page saves what is pending.
  const latest = useRef(cfg);
  const queued = useRef<OverlaySettings | null>(null);
  const timer = useRef<number | undefined>(undefined);

  useEffect(() => {
    let alive = true;
    backend.overlayStatus().then((s) => {
      if (!alive) return;
      if (s) {
        latest.current = s.settings;
        setStatus(s);
        setCfg(s.settings);
        // Dev aid (screenshots, manual QA): G2BOOST_AUTORUN=1 shows the overlay over the desktop.
        if (boot.autorun)
          backend.overlayTest(true).then(
            (st) => alive && setStatus(st),
            () => undefined,
          );
      }
      setLoaded(true);
    });
    const offs = [
      backend.on<OverlayFrame>("overlay:frame", (f) => alive && setLive(f)),
      backend.on<boolean>("overlay:visible", (v) => {
        if (!alive) return;
        setStatus((s) => (s ? { ...s, visible: v } : s));
        if (!v) setLive(null);
      }),
    ];
    return () => {
      alive = false;
      for (const off of offs) void off.then((f) => f());
    };
  }, []);

  const flush = useCallback(async () => {
    window.clearTimeout(timer.current);
    const next = queued.current;
    queued.current = null;
    if (!next || !inTauri) return;
    try {
      setStatus(await backend.overlaySet(next));
    } catch (e) {
      toast(errorText(e), { tone: "danger" });
    }
  }, [errorText]);
  useEffect(
    () => () => {
      void flush();
    },
    [flush],
  );
  const update = (patch: Partial<OverlaySettings>) => {
    const next = { ...latest.current, ...patch };
    latest.current = next;
    setCfg(next);
    queued.current = next;
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => void flush(), 250);
  };

  const visible = !!status?.visible;
  const test = async () => {
    setBusy(true);
    try {
      await flush();
      setStatus(await backend.overlayTest(!visible));
    } catch (e) {
      toast(errorText(e), { tone: "danger" });
    } finally {
      setBusy(false);
    }
  };

  const { mods, key } = splitHotkey(cfg.hotkey);
  const setHotkey = (nextMods: string[], nextKey: string) => {
    const spec = [...MODS.filter((m) => nextMods.includes(m)), nextKey].join("+");
    if (spec === "Alt+F4") {
      toast(t("overlay.hotkey.invalid_hotkey"), { tone: "warning" });
      return; // closes the game: the core refuses it too
    }
    update({ hotkey: spec });
  };
  const keyName = cfg.hotkey
    .split("+")
    .map((k) => KEY_CAP[k] ?? k)
    .join(" + ");
  const hotkeyError = status?.hotkey_error;
  const hotkeyText = hotkeyError
    ? hotkeyError === "hotkey_taken" || hotkeyError === "invalid_hotkey"
      ? t(`overlay.hotkey.${hotkeyError}` as I18nKey)
      : t("overlay.hotkey.error", { error: hotkeyError })
    : t(cfg.enabled ? "overlay.hotkey.press" : "overlay.hotkey.off", { key: keyName });

  const toggleMetric = (m: OverlayMetric) => {
    const on = new Set(cfg.metrics);
    if (on.has(m)) on.delete(m);
    else on.add(m);
    update({ metrics: ORDER.filter((x) => on.has(x)) });
  };

  const isLive = visible && !!live;
  const frame: OverlayFrame =
    isLive && live
      ? { ...live, config: cfg }
      : { ...SAMPLE, game: t("overlay.sample.game"), config: cfg };
  const elevated = status?.elevated ?? false;
  const locked = !loaded;

  return (
    <>
      <PageHeader
        title={t("nav.overlay")}
        subtitle={t("overlay.subtitle")}
        actions={
          <div className="flex items-center gap-3">
            {visible ? (
              <Badge tone="accent">
                <Eye className="mr-1 size-3" />
                {t("overlay.visible")}
              </Badge>
            ) : null}
            <Button variant="primary" tactile disabled={!inTauri || busy || locked} onClick={test}>
              {visible ? <EyeOff className="size-4" /> : <MonitorPlay className="size-4" />}
              {visible ? t("overlay.hide") : t("overlay.test")}
            </Button>
          </div>
        }
      />
      <PageBody>
        <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
          <Card className="animate-rise">
            <CardTitle
              right={
                <Badge tone={isLive ? "accent" : "neutral"}>
                  {isLive ? t("overlay.preview.live") : t("overlay.preview.sample")}
                </Badge>
              }
            >
              <MonitorPlay className="size-4" />
              {t("overlay.preview")}
            </CardTitle>
            <Stage
              frame={frame}
              corner={cfg.corner}
              onCorner={(c) => update({ corner: c })}
              disabled={locked}
            />
            <p className="mt-3 text-xs text-muted-foreground">{t("overlay.preview.hint")}</p>
          </Card>

          <Card className="animate-rise flex flex-col gap-4" style={rise(1)}>
            <div>
              <CardTitle
                className="mb-2"
                right={
                  <Switch
                    checked={cfg.enabled}
                    disabled={locked}
                    onCheckedChange={(v) => update({ enabled: v })}
                    aria-label={t("overlay.enabled")}
                  />
                }
              >
                <Gauge className="size-4" />
                {t("overlay.enabled")}
              </CardTitle>
              <p className="text-xs text-muted-foreground">{t("overlay.enabled.body")}</p>
            </div>

            <Field label={t("overlay.hotkey")}>
              <div className="flex flex-wrap items-center gap-2">
                {MODS.map((m) => (
                  <Chip
                    key={m}
                    pressed={mods.includes(m)}
                    disabled={locked}
                    onClick={() =>
                      setHotkey(mods.includes(m) ? mods.filter((x) => x !== m) : [...mods, m], key)
                    }
                  >
                    {m}
                  </Chip>
                ))}
                <Plus aria-hidden className="size-3.5 text-muted-foreground" />
                <select
                  className={selectClass}
                  aria-label={t("overlay.hotkey.key")}
                  value={key}
                  disabled={locked}
                  onChange={(e) => setHotkey(mods, e.target.value)}
                >
                  {KEYS.map((k) => (
                    <option key={k} value={k}>
                      {KEY_CAP[k] ?? k}
                    </option>
                  ))}
                </select>
              </div>
              <p
                className={cn(
                  "text-xs",
                  hotkeyError ? "text-destructive" : "text-muted-foreground",
                )}
              >
                {hotkeyText}
              </p>
            </Field>

            <Field label={t("overlay.style")}>
              <div className="flex gap-2">
                <Chip
                  pressed={cfg.style === "bar"}
                  disabled={locked}
                  onClick={() => update({ style: "bar" })}
                >
                  <RectangleHorizontal className="size-3.5" />
                  {t("overlay.style.bar")}
                </Chip>
                <Chip
                  pressed={cfg.style === "list"}
                  disabled={locked}
                  onClick={() => update({ style: "list" })}
                >
                  <List className="size-3.5" />
                  {t("overlay.style.list")}
                </Chip>
              </div>
            </Field>

            <Field label={t("overlay.opacity")} value={`${cfg.opacity} %`}>
              <input
                type="range"
                min={0}
                max={100}
                step={5}
                value={cfg.opacity}
                disabled={locked}
                aria-label={t("overlay.opacity")}
                onChange={(e) => update({ opacity: Number(e.target.value) })}
                className="w-full accent-primary"
              />
            </Field>

            <Field label={t("overlay.size")} value={`${cfg.size} %`}>
              <input
                type="range"
                min={75}
                max={150}
                step={5}
                value={cfg.size}
                disabled={locked}
                aria-label={t("overlay.size")}
                onChange={(e) => update({ size: Number(e.target.value) })}
                className="w-full accent-primary"
              />
            </Field>
          </Card>
        </div>

        <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(0,7fr)_minmax(0,5fr)]">
          <Card className="animate-rise" style={rise(2)}>
            <CardTitle help={t("overlay.metrics.help")}>
              <ListChecks className="size-4" />
              {t("overlay.metrics")}
            </CardTitle>
            <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
              {METRIC_GROUPS.map((g) => (
                <div key={g.label} className="flex flex-col gap-2">
                  <span className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
                    {t(g.label)}
                  </span>
                  <div className="flex flex-wrap gap-2">
                    {g.metrics.map((m) => {
                      const on = cfg.metrics.includes(m);
                      return (
                        <Chip
                          key={m}
                          pressed={on}
                          disabled={locked || (on && cfg.metrics.length === 1)}
                          onClick={() => toggleMetric(m)}
                        >
                          {on ? <Check className="size-3.5 text-primary" /> : null}
                          {t(`overlay.metric.${m}` as I18nKey)}
                        </Chip>
                      );
                    })}
                  </div>
                  {g.note ? (
                    <p className="text-[11px] leading-relaxed text-muted-foreground">{t(g.note)}</p>
                  ) : null}
                </div>
              ))}
            </div>
          </Card>

          <Card className="animate-rise" style={rise(3)}>
            <CardTitle>
              <ShieldCheck className="size-4" />
              {t("overlay.how")}
            </CardTitle>
            <ul className="flex flex-col gap-3 text-xs leading-relaxed text-muted-foreground">
              <li className="flex gap-2.5">
                <MonitorPlay className="mt-0.5 size-4 shrink-0 text-foreground/70" />
                {t("overlay.how.windowed")}
              </li>
              <li className="flex gap-2.5">
                <ShieldCheck className="mt-0.5 size-4 shrink-0 text-foreground/70" />
                {t("overlay.how.anticheat")}
              </li>
              <li className="flex gap-2.5">
                <MousePointerClick className="mt-0.5 size-4 shrink-0 text-foreground/70" />
                {t("overlay.how.focus")}
              </li>
              <li className="flex gap-2.5">
                <Gauge className="mt-0.5 size-4 shrink-0 text-foreground/70" />
                {t("overlay.how.vulkan")}
              </li>
              <li className="flex flex-col gap-2 rounded-md border border-border bg-foreground/3 p-3">
                <span className={elevated ? "text-success" : "text-foreground"}>
                  {elevated ? t("overlay.how.admin_ok") : t("overlay.how.admin")}
                </span>
                {elevated ? null : (
                  <span>
                    <ElevateButton route="overlay" />
                  </span>
                )}
              </li>
            </ul>
          </Card>
        </div>
      </PageBody>
    </>
  );
}
