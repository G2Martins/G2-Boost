import { ChevronDown, ChevronUp, MousePointer2, RotateCcw } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { PageBody, PageHeader } from "@/components/shell/page-header";
import { MouseFigure } from "@/components/tools/mouse-figure";
import { Stat } from "@/components/tools/stat";
import { LiveLine } from "@/components/ui/live-line";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import { type I18nKey, useI18n } from "@/lib/i18n";
import {
  BOUNCE_MS,
  BUTTONS,
  type ButtonId,
  type ButtonStat,
  CHART_POINTS,
  emptyButtons,
  pressButton,
  RateMeter,
  type RateSnapshot,
  rulerPosition,
  STANDARD_RATES,
} from "@/lib/mouse";
import { cn } from "@/lib/utils";

/**
 * `pointerrawupdate` carries every report of the mouse; without it `pointermove` is grouped per
 * frame. The browser only fires it in a secure context (`localhost` counts, so the app does).
 */
const RAW =
  typeof window !== "undefined" && window.isSecureContext && "onpointerrawupdate" in window;

export function MouseTestPage() {
  const { t } = useI18n();
  return (
    <PageHeader
      title={t("nav.mousetest")}
      subtitle={t("tools.mouse.subtitle")}
      tabs={
        <Tabs defaultValue="polling">
          <TabsList>
            <TabsTrigger value="polling">{t("tools.mouse.poll.title")}</TabsTrigger>
            <TabsTrigger value="buttons">{t("tools.mouse.buttons.title")}</TabsTrigger>
          </TabsList>
          <TabsContent value="polling">
            <PageBody className="px-0 pb-0">
              <PollingCard />
            </PageBody>
          </TabsContent>
          <TabsContent value="buttons">
            <PageBody className="px-0 pb-0">
              <ButtonsCard />
            </PageBody>
          </TabsContent>
        </Tabs>
      }
    />
  );
}

/* ================= polling rate ================= */

function PollingCard() {
  const { t, locale } = useI18n();
  const [meter] = useState(() => new RateMeter());
  const pad = useRef<HTMLDivElement>(null);
  const dirty = useRef(false);
  const [snap, setSnap] = useState<RateSnapshot>(() => meter.snapshot());

  useEffect(() => {
    const el = pad.current;
    if (!el) return;
    // 1000 reports a second must not become 1000 renders: the events only fill the meter and
    // the screen follows ten times a second.
    const onMove = (ev: Event) => {
      const e = ev as PointerEvent;
      if (e.pointerType !== "mouse") return;
      const batch = e.getCoalescedEvents?.() ?? [];
      for (const c of batch.length ? batch : [e]) meter.push(c.timeStamp);
      dirty.current = true;
    };
    const type = RAW ? "pointerrawupdate" : "pointermove";
    el.addEventListener(type, onMove);
    const tick = window.setInterval(() => {
      if (!dirty.current) return;
      dirty.current = false;
      setSnap(meter.snapshot());
    }, 100);
    return () => {
      el.removeEventListener(type, onMove);
      window.clearInterval(tick);
    };
  }, [meter]);

  const reset = () => {
    meter.reset();
    dirty.current = false;
    setSnap(meter.snapshot());
  };

  const fmt = useMemo(() => {
    const whole = new Intl.NumberFormat(locale, { maximumFractionDigits: 0, useGrouping: false });
    const fine = new Intl.NumberFormat(locale, { maximumFractionDigits: 2 });
    return {
      hz: (v: number | null) => (v === null ? "—" : whole.format(v)),
      ms: (v: number | null) => (v === null ? "—" : fine.format(v)),
      n: (v: number) => new Intl.NumberFormat(locale).format(v),
    };
  }, [locale]);

  return (
    <Card className="animate-rise">
      <CardTitle
        help={t("tools.mouse.poll.help")}
        right={
          <span className="flex items-center gap-2">
            <Badge
              tone={RAW ? "neutral" : "warning"}
              title={t(RAW ? "tools.mouse.source.raw.help" : "tools.mouse.source.grouped.help")}
              className="normal-case tracking-normal"
            >
              {t(RAW ? "tools.mouse.source.raw" : "tools.mouse.source.grouped")}
            </Badge>
            <Button variant="outline" size="sm" onClick={reset}>
              <RotateCcw className="size-4" />
              {t("tools.reset")}
            </Button>
          </span>
        }
      >
        {t("tools.mouse.poll.title")}
      </CardTitle>

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[minmax(0,1fr)_17rem]">
        <div className="min-w-0 space-y-4">
          <section
            ref={pad}
            aria-label={t("tools.mouse.pad.label")}
            data-testid="mouse-pad"
            className="frame-ticks relative h-52 cursor-crosshair touch-none select-none overflow-hidden rounded-md border border-border-strong bg-background/60"
          >
            <PadRings />
            <div
              className={cn(
                "pointer-events-none absolute inset-0 flex flex-col items-center justify-center gap-2 px-6 text-center motion-safe:transition-opacity motion-safe:duration-500",
                snap.reports > 0 && "opacity-25",
              )}
            >
              <MousePointer2 className="size-5 text-muted-foreground" />
              <p className="font-display text-sm font-semibold">{t("tools.mouse.pad.title")}</p>
              <p className="max-w-md text-xs text-muted-foreground">{t("tools.mouse.pad.body")}</p>
            </div>
          </section>
          <div>
            <p className="mb-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              {t("tools.mouse.chart")}
            </p>
            <LiveLine
              series={[{ values: snap.recent, color: "hsl(var(--chart-1))" }]}
              window={CHART_POINTS}
              format={(v) => (v < 10 ? v.toFixed(1) : Math.round(v).toString())}
              label={t("tools.mouse.chart.label")}
              className="h-auto w-full"
            />
          </div>
        </div>

        <div className="space-y-5">
          <div className="frame-ticks rounded-md bg-background/60 p-4">
            <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              {t("tools.mouse.now")}
            </p>
            <p
              data-testid="mouse-now"
              className="num mt-1 font-display text-6xl font-semibold leading-none"
            >
              {fmt.hz(snap.hzNow)}
              <span className="ml-2 text-lg font-normal text-muted-foreground">Hz</span>
            </p>
          </div>
          <div className="grid grid-cols-2 gap-x-4 gap-y-4">
            <Stat label={t("tools.mouse.max")} value={fmt.hz(snap.hzMax)} unit="Hz" />
            <Stat label={t("tools.mouse.avg")} value={fmt.hz(snap.hzAvg)} unit="Hz" />
            <Stat
              label={t("tools.mouse.response")}
              value={fmt.ms(snap.avgMs)}
              unit="ms"
              help={t("tools.mouse.response.help")}
            />
            <Stat
              label={t("tools.mouse.jitter")}
              value={fmt.ms(snap.jitterMs)}
              unit="ms"
              help={t("tools.mouse.jitter.help")}
            />
            <Stat
              label={t("tools.mouse.samples")}
              value={snap.reports ? fmt.n(snap.reports) : "—"}
              className="col-span-2"
            />
          </div>
          <Ruler avg={snap.hzAvg} max={snap.hzMax} />
        </div>
      </div>
      <p className="mt-4 text-xs text-muted-foreground">
        {t("tools.mouse.note", { stalls: snap.stalls })}
      </p>
    </Card>
  );
}

/** Dashed rings behind the hint: "move in circles". Decorative, slowly turning. */
function PadRings() {
  return (
    <div
      aria-hidden
      className="absolute inset-0 flex items-center justify-center motion-safe:animate-[spin_60s_linear_infinite]"
    >
      <svg viewBox="0 0 200 200" className="h-[135%] w-auto" fill="none" aria-hidden>
        {[90, 62, 34].map((r) => (
          <circle
            key={r}
            cx={100}
            cy={100}
            r={r}
            stroke="hsl(var(--border-strong))"
            strokeWidth={1}
            strokeDasharray="2 7"
          />
        ))}
      </svg>
    </div>
  );
}

/** Log ruler of the usual rates: a filled marker for the average, a pale one for the maximum. */
function Ruler({ avg, max }: { avg: number | null; max: number | null }) {
  const mark = (at: number | null, cls: string) =>
    at === null ? null : (
      <span
        className={cn(
          "absolute top-0 h-5 w-0.5 -translate-x-1/2 rounded-full motion-safe:transition-[left] motion-safe:duration-300",
          cls,
        )}
        style={{ left: `${rulerPosition(at) * 100}%` }}
      />
    );
  return (
    <div className="px-3" aria-hidden>
      <div className="relative h-10">
        <span className="absolute inset-x-0 top-2.5 h-px bg-border-strong" />
        {STANDARD_RATES.map((r) => (
          <span
            key={r}
            className="absolute top-2 flex -translate-x-1/2 flex-col items-center"
            style={{ left: `${rulerPosition(r) * 100}%` }}
          >
            <span className="h-1.5 w-px bg-border-strong" />
            <span className="num mt-1 text-[10px] text-muted-foreground">{r}</span>
          </span>
        ))}
        {mark(max, "bg-foreground/45")}
        {mark(avg, "bg-primary shadow-[0_0_8px_hsl(var(--primary)/0.55)]")}
      </div>
    </div>
  );
}

/* ================= buttons and wheel ================= */

const NONE_DOWN: Record<ButtonId, boolean> = {
  left: false,
  middle: false,
  right: false,
  back: false,
  forward: false,
};

const BUTTON_LABEL: Record<ButtonId, I18nKey> = {
  left: "tools.mouse.btn.left",
  middle: "tools.mouse.btn.middle",
  right: "tools.mouse.btn.right",
  back: "tools.mouse.btn.back",
  forward: "tools.mouse.btn.forward",
};

type Wheel = { up: number; down: number; dir: "up" | "down" | null };
const NO_WHEEL: Wheel = { up: 0, down: 0, dir: null };

function ButtonsCard() {
  const { t, locale } = useI18n();
  const pad = useRef<HTMLDivElement>(null);
  const flash = useRef<number | undefined>(undefined);
  const [down, setDown] = useState(NONE_DOWN);
  const [stats, setStats] = useState<ButtonStat[]>(emptyButtons);
  const [wheel, setWheel] = useState(NO_WHEEL);

  useEffect(() => {
    const el = pad.current;
    if (!el) return;
    const onDown = (e: MouseEvent) => {
      e.preventDefault(); // no text selection, no middle-click autoscroll
      const id = BUTTONS[e.button];
      if (!id) return;
      setDown((d) => ({ ...d, [id]: true }));
      setStats((s) => pressButton(s, e.button, e.timeStamp));
    };
    // Released anywhere: the button must not stay lit if the pointer left the area.
    const onUp = (e: MouseEvent) => {
      const id = BUTTONS[e.button];
      if (!id) return;
      setDown((d) => ({ ...d, [id]: false }));
      if (el.contains(e.target as Node)) e.preventDefault(); // back and forward would navigate
    };
    const swallow = (e: Event) => e.preventDefault(); // right click menu, auxiliary click
    const onWheel = (e: WheelEvent) => {
      e.preventDefault(); // the page must not scroll under the test
      const dir = e.deltaY < 0 ? "up" : e.deltaY > 0 ? "down" : null;
      if (!dir) return;
      setWheel((w) => ({ ...w, [dir]: w[dir] + 1, dir }));
      window.clearTimeout(flash.current);
      flash.current = window.setTimeout(() => setWheel((w) => ({ ...w, dir: null })), 160);
    };
    el.addEventListener("mousedown", onDown);
    window.addEventListener("mouseup", onUp);
    el.addEventListener("contextmenu", swallow);
    el.addEventListener("auxclick", swallow);
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => {
      el.removeEventListener("mousedown", onDown);
      window.removeEventListener("mouseup", onUp);
      el.removeEventListener("contextmenu", swallow);
      el.removeEventListener("auxclick", swallow);
      el.removeEventListener("wheel", onWheel);
      window.clearTimeout(flash.current);
    };
  }, []);

  const reset = () => {
    setDown(NONE_DOWN);
    setStats(emptyButtons());
    setWheel(NO_WHEEL);
  };

  const total = stats.reduce((n, s) => n + s.downs, 0);
  const bouncing = BUTTONS.map((id, i) => ({ id, stat: stats[i] })).filter(
    (b): b is { id: ButtonId; stat: ButtonStat } => !!b.stat && b.stat.bounces > 0,
  );
  const num = (v: number) => new Intl.NumberFormat(locale).format(v);

  return (
    <Card className="animate-rise">
      <CardTitle
        right={
          <Button variant="outline" size="sm" onClick={reset}>
            <RotateCcw className="size-4" />
            {t("tools.reset")}
          </Button>
        }
      >
        {t("tools.mouse.buttons.title")}
      </CardTitle>
      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,22rem)]">
        <section
          ref={pad}
          aria-label={t("tools.mouse.buttons.label")}
          data-testid="click-pad"
          className="frame-ticks relative flex min-h-80 touch-none select-none flex-col items-center justify-center gap-3 rounded-md border border-border-strong bg-background/60 px-6 py-5 text-center"
        >
          <MouseFigure down={down} wheel={wheel.dir} />
          <p className="font-display text-sm font-semibold">{t("tools.mouse.buttons.pad")}</p>
          <p className="max-w-sm text-xs text-muted-foreground">
            {t("tools.mouse.buttons.pad.body")}
          </p>
        </section>

        <div className="space-y-5">
          <div>
            <p className="mb-1 flex justify-between text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              <span>{t("tools.mouse.clicks")}</span>
              <span>{num(total)}</span>
            </p>
            <ul className="divide-y divide-border/60 text-[13px]">
              {BUTTONS.map((id, i) => {
                const s = stats[i];
                return (
                  <li key={id} className="flex items-center gap-3 py-2">
                    <span
                      aria-hidden
                      className={cn(
                        "size-2 shrink-0 rounded-full",
                        down[id] ? "bg-primary" : "bg-foreground/15",
                      )}
                    />
                    <span className="flex-1">{t(BUTTON_LABEL[id])}</span>
                    {s && s.bounces > 0 ? (
                      <Badge tone="danger" className="normal-case tracking-normal">
                        {s.bounces}x
                      </Badge>
                    ) : null}
                    <span data-testid={`count-${id}`} className="num w-10 text-right font-semibold">
                      {s?.downs ?? 0}
                    </span>
                  </li>
                );
              })}
            </ul>
          </div>

          <div>
            <p className="mb-2 flex items-center gap-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
              {t("tools.mouse.wheel")}
            </p>
            <div className="grid grid-cols-2 gap-3 text-[13px]" title={t("tools.mouse.wheel.help")}>
              <span className="flex items-center gap-2 rounded-md bg-foreground/5 px-3 py-2">
                <ChevronUp className="size-4 text-muted-foreground" />
                <span className="flex-1">{t("tools.mouse.wheel.up")}</span>
                <b data-testid="wheel-up" className="num">
                  {wheel.up}
                </b>
              </span>
              <span className="flex items-center gap-2 rounded-md bg-foreground/5 px-3 py-2">
                <ChevronDown className="size-4 text-muted-foreground" />
                <span className="flex-1">{t("tools.mouse.wheel.down")}</span>
                <b data-testid="wheel-down" className="num">
                  {wheel.down}
                </b>
              </span>
            </div>
          </div>

          <div
            role="status"
            data-testid="bounce-status"
            className={cn(
              "rounded-md border p-3 text-xs leading-relaxed",
              bouncing.length
                ? "border-destructive/40 bg-destructive/8"
                : "border-border bg-foreground/4 text-muted-foreground",
            )}
          >
            {bouncing.length ? (
              <>
                <p className="mb-1 font-semibold text-destructive">
                  {t("tools.mouse.bounce.title")}
                </p>
                {bouncing.map(({ id, stat }) => (
                  <p key={id}>
                    {t("tools.mouse.bounce.body", {
                      button: t(BUTTON_LABEL[id]),
                      ms: BOUNCE_MS,
                      count: stat.bounces,
                      gap: Math.round(stat.gapMs ?? 0),
                    })}
                  </p>
                ))}
              </>
            ) : total >= 6 ? (
              t("tools.mouse.bounce.ok", { count: total })
            ) : (
              t("tools.mouse.bounce.idle")
            )}
          </div>
        </div>
      </div>
    </Card>
  );
}
