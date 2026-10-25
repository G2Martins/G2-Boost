import {
  ArrowDown,
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  Blend,
  Contrast,
  Expand,
  Grid3x3,
  type LucideIcon,
  Moon,
  Move,
  Palette,
  Sun,
} from "lucide-react";
import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { createPortal, flushSync } from "react-dom";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  GROUPS,
  type GroupId,
  type MotionResult,
  Pattern,
  PREVIEW,
} from "@/components/tools/monitor-patterns";
import { Button, Card, CardTitle, MetricRow } from "@/components/ui/primitives";
import { useI18n } from "@/lib/i18n";
import { nearestStandardHz } from "@/lib/refresh";
import { cn } from "@/lib/utils";

const ICON: Record<GroupId, LucideIcon> = {
  solid: Palette,
  blacks: Moon,
  whites: Sun,
  gradient: Blend,
  uniformity: Contrast,
  sharpness: Grid3x3,
  motion: Move,
};

type Run = { group: number; step: number };

/** How long the hint stays after the last key or mouse move (ms). */
const HINT_MS = 3500;

function readScreen() {
  const dpr = window.devicePixelRatio || 1;
  return {
    w: Math.round(screen.width * dpr),
    h: Math.round(screen.height * dpr),
    scale: Math.round(dpr * 100),
    depth: screen.colorDepth,
  };
}

function useScreen() {
  const [info, setInfo] = useState(readScreen);
  useEffect(() => {
    const on = () => setInfo(readScreen());
    window.addEventListener("resize", on); // moving the window to another monitor resizes it
    return () => window.removeEventListener("resize", on);
  }, []);
  return info;
}

export function MonitorTestPage() {
  const { t, locale } = useI18n();
  const screenInfo = useScreen();
  const [run, setRun] = useState<Run | null>(null);
  const [last, setLast] = useState<MotionResult | null>(null);
  const [hint, setHint] = useState(true);
  const surface = useRef<HTMLDivElement>(null);
  const hintTimer = useRef<number | undefined>(undefined);
  const opener = useRef<HTMLElement | null>(null);
  const active = run !== null;

  // Back on the launcher, keyboard focus returns to the button that opened the test.
  useEffect(() => {
    if (!active) opener.current?.focus();
  }, [active]);

  const poke = useCallback(() => {
    setHint(true);
    window.clearTimeout(hintTimer.current);
    hintTimer.current = window.setTimeout(() => setHint(false), HINT_MS);
  }, []);

  const start = (group: number) => {
    // The surface has to exist before it can go full screen, and the request has to happen
    // while the click still counts as a user gesture.
    opener.current = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    flushSync(() => setRun({ group, step: 0 }));
    const el = surface.current;
    el?.focus();
    // Refused (or not available)? The surface still covers the window and Esc still leaves.
    el?.requestFullscreen?.({ navigationUI: "hide" })?.catch(() => undefined);
    poke();
  };

  const stop = useCallback(() => {
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => undefined);
    setRun(null);
  }, []);

  /** `dGroup` jumps between tests (and starts at their first screen); `dStep` moves inside one. */
  const go = useCallback(
    (dGroup: number, dStep: number) => {
      setRun((r) => {
        if (!r) return r;
        const n = GROUPS.length;
        const group = dGroup ? (r.group + dGroup + n) % n : r.group;
        const count = GROUPS[group]?.steps.length ?? 1;
        return { group, step: dGroup ? 0 : (r.step + dStep + count) % count };
      });
      poke();
    },
    [poke],
  );

  useEffect(() => {
    if (!active) return;
    const el = surface.current;
    const onKey = (e: KeyboardEvent) => {
      switch (e.key) {
        case "ArrowRight":
        case "PageDown":
        case " ":
          go(0, 1);
          break;
        case "ArrowLeft":
        case "PageUp":
          go(0, -1);
          break;
        case "ArrowDown":
          go(1, 0);
          break;
        case "ArrowUp":
          go(-1, 0);
          break;
        case "Escape":
          stop();
          break;
        default:
          return;
      }
      e.preventDefault();
    };
    // Esc in real full screen is taken by the browser, which then tells us it left.
    const onFullscreen = () => {
      if (!document.fullscreenElement) setRun(null);
    };
    const onClick = () => go(0, 1);
    const onMove = () => poke();
    window.addEventListener("keydown", onKey);
    document.addEventListener("fullscreenchange", onFullscreen);
    el?.addEventListener("click", onClick);
    el?.addEventListener("pointermove", onMove);
    return () => {
      window.removeEventListener("keydown", onKey);
      document.removeEventListener("fullscreenchange", onFullscreen);
      el?.removeEventListener("click", onClick);
      el?.removeEventListener("pointermove", onMove);
      window.clearTimeout(hintTimer.current);
    };
  }, [active, go, stop, poke]);

  const group = run ? GROUPS[run.group] : undefined;
  const step = run && group ? group.steps[run.step] : undefined;
  const num = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
  const standard = nearestStandardHz(last?.hz ?? null);

  return (
    <>
      <PageHeader title={t("nav.monitortest")} subtitle={t("tools.monitor.subtitle")} />
      <PageBody>
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-[minmax(0,1fr)_21rem]">
          <Card className="animate-rise frame-ticks flex flex-col gap-4">
            <CardTitle>{t("tools.monitor.start.title")}</CardTitle>
            <p className="max-w-xl text-[13px] leading-relaxed text-muted-foreground">
              {t("tools.monitor.start.body")}
            </p>
            <div>
              <Button variant="primary" tactile size="lg" onClick={() => start(0)}>
                <Expand className="size-4" />
                {t("tools.monitor.start.button")}
              </Button>
            </div>
            <div className="flex flex-wrap items-center gap-x-5 gap-y-2 text-xs text-muted-foreground">
              <KeyHelp
                keys={[
                  <ArrowLeft key="l" className="size-3" />,
                  <ArrowRight key="r" className="size-3" />,
                ]}
                label={t("tools.monitor.key.pattern")}
              />
              <KeyHelp
                keys={[
                  <ArrowUp key="u" className="size-3" />,
                  <ArrowDown key="d" className="size-3" />,
                ]}
                label={t("tools.monitor.key.test")}
              />
              <KeyHelp keys={[t("tools.monitor.key.esc")]} label={t("tools.monitor.key.exit")} />
            </div>
            <p className="text-xs text-muted-foreground">{t("tools.monitor.key.click")}</p>
          </Card>

          <Card className="animate-rise" style={rise(1)}>
            <CardTitle>{t("tools.monitor.screen.title")}</CardTitle>
            <div className="divide-y divide-border text-[13px]">
              <MetricRow
                label={t("tools.monitor.screen.resolution")}
                value={`${screenInfo.w} × ${screenInfo.h} px`}
              />
              <MetricRow label={t("tools.monitor.screen.scale")} value={`${screenInfo.scale}%`} />
              <MetricRow
                label={t("tools.monitor.screen.colors")}
                value={t("tools.monitor.screen.colors.value", { bits: screenInfo.depth })}
              />
              <MetricRow
                label={t("tools.monitor.screen.rate")}
                value={
                  last
                    ? `${num.format(last.hz)} Hz${standard ? ` (${standard})` : ""}`
                    : t("tools.monitor.screen.rate.none")
                }
              />
            </div>
            {last ? (
              <p className="mt-3 text-xs text-muted-foreground">
                {t("tools.monitor.screen.rate.note", {
                  seconds: Math.round(last.seconds),
                  dropped: last.dropped,
                })}
              </p>
            ) : null}
          </Card>
        </div>

        <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
          {GROUPS.map((g, i) => {
            const Icon = ICON[g.id];
            return (
              <button
                key={g.id}
                type="button"
                onClick={() => start(i)}
                className="panel panel-hover animate-rise press flex flex-col gap-3 p-4 text-left"
                style={rise(i + 2)}
              >
                <span className="flex items-center gap-3">
                  <span className="flex size-9 shrink-0 items-center justify-center rounded-md border border-border bg-card-hover text-muted-foreground">
                    <Icon className="size-4" />
                  </span>
                  <span className="font-display text-[14px] font-semibold leading-tight">
                    {t(g.title)}
                  </span>
                </span>
                <span
                  aria-hidden
                  className="block h-6 rounded-sm ring-1 ring-border-strong"
                  style={{ background: PREVIEW[g.id] }}
                />
                <span className="text-xs leading-relaxed text-muted-foreground">{t(g.desc)}</span>
                <span className="mt-auto flex items-center gap-1.5 pt-1 text-xs font-semibold text-primary">
                  <Expand className="size-3.5" />
                  {t("tools.monitor.open")}
                </span>
              </button>
            );
          })}
        </div>
        <p className="text-xs text-muted-foreground">{t("tools.monitor.motion.notice")}</p>
      </PageBody>

      {run && group && step
        ? createPortal(
            <div
              ref={surface}
              role="application"
              aria-label={t("tools.monitor.surface")}
              tabIndex={-1}
              className="fixed inset-0 z-[100] bg-background outline-none"
              style={{ cursor: hint ? "default" : "none" }}
            >
              <Pattern group={group.id} step={run.step} onMotion={setLast} />
              <div
                aria-hidden={!hint}
                className={cn(
                  "pointer-events-none absolute inset-x-0 bottom-8 flex justify-center motion-safe:transition-opacity motion-safe:duration-700",
                  hint ? "opacity-100" : "opacity-0",
                )}
              >
                <div className="rounded-lg border border-border-strong bg-background/90 px-4 py-3 text-center shadow-xl">
                  <p className="font-display text-sm font-semibold">{t(group.title)}</p>
                  {group.steps.length > 1 ? (
                    <p className="text-xs text-muted-foreground">
                      {t("tools.monitor.hint.step", {
                        name: t(step.name),
                        n: run.step + 1,
                        total: group.steps.length,
                      })}
                    </p>
                  ) : null}
                  <div className="mt-2 flex items-center justify-center gap-5 text-[11px] text-muted-foreground">
                    <KeyHelp
                      keys={[
                        <ArrowLeft key="l" className="size-3" />,
                        <ArrowRight key="r" className="size-3" />,
                      ]}
                      label={t("tools.monitor.key.pattern")}
                    />
                    <KeyHelp
                      keys={[
                        <ArrowUp key="u" className="size-3" />,
                        <ArrowDown key="d" className="size-3" />,
                      ]}
                      label={t("tools.monitor.key.test")}
                    />
                    <KeyHelp
                      keys={[t("tools.monitor.key.esc")]}
                      label={t("tools.monitor.key.exit")}
                    />
                  </div>
                </div>
              </div>
              <span className="sr-only" aria-live="polite">
                {t(group.title)}: {t(step.name)}
              </span>
            </div>,
            document.body,
          )
        : null}
    </>
  );
}

function KeyHelp({ keys, label }: { keys: ReactNode[]; label: string }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      {keys.map((k, i) => (
        <kbd
          // biome-ignore lint/suspicious/noArrayIndexKey: a fixed pair of key caps
          key={i}
          className="inline-flex h-6 min-w-6 items-center justify-center rounded border border-border-strong bg-foreground/6 px-1.5 text-[11px] font-semibold text-foreground"
        >
          {k}
        </kbd>
      ))}
      {label}
    </span>
  );
}
