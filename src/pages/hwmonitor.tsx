import {
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  CircuitBoard,
  Cpu,
  FileSpreadsheet,
  Gpu,
  HardDrive,
  type LucideIcon,
  MemoryStick,
  Network,
  RotateCcw,
  Thermometer,
} from "lucide-react";
import { type ReactNode, type RefObject, useCallback, useEffect, useRef, useState } from "react";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { reducedMotion } from "@/components/ui/count-up";
import { Badge, Button, Card, EmptyState, Skeleton } from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import { backend, type HwDeviceKind, type HwSensor, type HwSnapshot, inTauri } from "@/lib/backend";
import {
  csvRows,
  deviceIds,
  formatSensor,
  type Labelled,
  loadCollapsed,
  position,
  type Scope,
  type Stat,
  saveCollapsed,
  trackStats,
} from "@/lib/hw";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const ICONS: Record<HwDeviceKind, LucideIcon> = {
  cpu: Cpu,
  gpu: Gpu,
  memory: MemoryStick,
  disk: HardDrive,
  network: Network,
  external: CircuitBoard,
};

const CELL = "border-t border-border/40";

let flashColor: string | null = null;
/** The accent at 16%, resolved once from the token (animations do not reliably take var()). */
function flashFrom(): string {
  if (flashColor === null) {
    const v = getComputedStyle(document.documentElement).getPropertyValue("--primary").trim();
    flashColor = v ? `hsl(${v} / 0.16)` : "transparent";
  }
  return flashColor;
}

/** A short tint that fades when the shown text changes (not on first paint, never under reduced motion). */
function useFlash(ref: RefObject<HTMLElement | null>, text: string) {
  const first = useRef(true);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `text` is the trigger, the effect never reads it
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    const el = ref.current;
    if (!el || reducedMotion() || typeof el.animate !== "function") return;
    el.animate([{ backgroundColor: flashFrom() }, { backgroundColor: "transparent" }], {
      duration: 700,
      easing: "cubic-bezier(0.16, 1, 0.3, 1)",
    });
  }, [text, ref]);
}

export function HwMonitorPage() {
  const { t, locale } = useI18n();
  const [snap, setSnap] = useState<HwSnapshot | null>(null);
  const stats = useRef(new Map<string, Stat>());
  const [collapsed, setCollapsed] = useState(loadCollapsed);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let alive = true;
    let off: (() => void) | null = null;
    let running = false;
    // A window hidden in the tray keeps this page mounted: sample only while it can be seen.
    const sync = () => {
      const visible = document.visibilityState !== "hidden";
      if (visible && !running) backend.hwStart();
      else if (!visible && running) backend.hwStop();
      running = visible;
    };
    sync();
    document.addEventListener("visibilitychange", sync);
    backend
      .on<HwSnapshot>("hw:tick", (s) => {
        if (!alive) return;
        trackStats(stats.current, s);
        setSnap(s);
      })
      .then((u) => {
        if (alive) off = u;
        else u();
      });
    return () => {
      alive = false;
      document.removeEventListener("visibilitychange", sync);
      off?.();
      if (running) backend.hwStop();
    };
  }, []);
  useEffect(() => saveCollapsed(collapsed), [collapsed]);

  const label = useCallback(
    (scope: Scope, x: Labelled) =>
      x.key ? t(`hw.${scope}.${x.key}` as I18nKey, { n: x.n ?? 0 }) : x.label,
    [t],
  );
  const toggle = (id: string) =>
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const resetStats = () => {
    stats.current.clear();
    if (snap) {
      trackStats(stats.current, snap);
      setSnap({ ...snap });
    }
  };
  const exportCsv = async () => {
    if (!snap) return;
    setSaving(true);
    try {
      const path = await backend.hwExportCsv({
        locale,
        header: [
          t("hw.csv.device"),
          t("hw.csv.group"),
          t("hw.col.sensor"),
          t("hw.col.value"),
          t("hw.col.min"),
          t("hw.col.max"),
          t("hw.csv.unit"),
        ],
        rows: csvRows(snap, stats.current, label),
      });
      toast(t("hw.csv.saved", { name: path.split("\\").pop() ?? path }), { tone: "success" });
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      toast(
        msg === "too_many_rows"
          ? t("hw.csv.error.too_many_rows")
          : t("hw.csv.error", { error: msg }),
        { tone: "danger" },
      );
    } finally {
      setSaving(false);
    }
  };

  const rows: ReactNode[] = [];
  for (const d of snap?.devices ?? []) {
    const open = !collapsed.has(d.id);
    rows.push(
      <DeviceRow
        key={d.id}
        icon={ICONS[d.kind]}
        name={label("d", d)}
        count={d.groups.reduce((n, g) => n + g.sensors.length, 0)}
        open={open}
        onToggle={() => toggle(d.id)}
      />,
    );
    if (!open) continue;
    for (const g of d.groups) {
      const groupOpen = !collapsed.has(g.id);
      rows.push(
        <GroupRow
          key={g.id}
          name={label("g", g)}
          count={g.sensors.length}
          open={groupOpen}
          onToggle={() => toggle(g.id)}
        />,
      );
      if (!groupOpen) continue;
      for (const s of g.sensors) {
        rows.push(
          <SensorRow
            key={s.id}
            sensor={s}
            name={label("s", s)}
            stat={stats.current.get(s.id)}
            locale={locale}
          />,
        );
      }
    }
    if (d.kind === "cpu" && !snap?.cpu_temp) {
      rows.push(<HintRow key={`${d.id}/hint`} text={t("dash.monitor.cpu_temp.na")} />);
    }
  }

  return (
    <PageHeader
      title={t("nav.hwmonitor")}
      subtitle={t("hw.subtitle")}
      tabs={
        <PageBody className="px-0 pb-0">
          {!inTauri ? (
            <Card className="animate-rise">
              <EmptyState icon={<CircuitBoard className="size-5" />} title={t("common.app_only")} />
            </Card>
          ) : (
            <Card className="frame-ticks animate-rise p-0" style={rise(1)}>
              <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b border-border px-4 py-3">
                <span className="flex items-center gap-2 text-xs">
                  <span
                    aria-hidden
                    className="size-2 rounded-full bg-success motion-safe:animate-pulse-dot"
                  />
                  <span className="num font-semibold">
                    {snap ? t("hw.count", { count: snap.count }) : t("hw.reading")}
                  </span>
                  <span className="text-muted-foreground">{t("hw.every")}</span>
                </span>
                {snap?.external ? (
                  <Badge tone="success">
                    {t("hw.source", { name: t(`hw.source.${snap.external}` as I18nKey) })}
                  </Badge>
                ) : null}
                <div className="ml-auto flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!snap}
                    onClick={() => setCollapsed(new Set(deviceIds(snap)))}
                  >
                    <ChevronsDownUp className="size-4" />
                    {t("hw.collapse_all")}
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!snap}
                    onClick={() => setCollapsed(new Set())}
                  >
                    <ChevronsUpDown className="size-4" />
                    {t("hw.expand_all")}
                  </Button>
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={!snap}
                    title={t("hw.reset.hint")}
                    onClick={resetStats}
                  >
                    <RotateCcw className="size-4" />
                    {t("hw.reset")}
                  </Button>
                  <Button
                    size="sm"
                    variant="primary"
                    disabled={!snap || saving}
                    onClick={exportCsv}
                  >
                    <FileSpreadsheet className="size-4" />
                    {t("hw.csv")}
                  </Button>
                </div>
              </div>
              {/* The list scrolls inside the frame so the column titles stay put (the banner above
                  clips overflow, which would stop a sticky header from sticking to the page). */}
              <div className="max-h-[calc(100vh-19rem)] min-h-64 overflow-auto">
                <table
                  aria-label={t("nav.hwmonitor")}
                  aria-busy={!snap}
                  className="w-full table-fixed select-text border-separate border-spacing-0 text-[13px]"
                >
                  <colgroup>
                    <col />
                    <col className="w-48" />
                    <col className="w-28" />
                    <col className="w-28" />
                  </colgroup>
                  <thead>
                    <tr className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                      <th className="sticky top-0 z-10 bg-surface px-4 py-2 text-left">
                        {t("hw.col.sensor")}
                      </th>
                      <th className="sticky top-0 z-10 bg-surface px-3 py-2 text-right">
                        {t("hw.col.value")}
                      </th>
                      <th className="sticky top-0 z-10 bg-surface px-3 py-2 text-right">
                        {t("hw.col.min")}
                      </th>
                      <th className="sticky top-0 z-10 bg-surface px-3 py-2 text-right">
                        {t("hw.col.max")}
                      </th>
                    </tr>
                  </thead>
                  <tbody>{snap ? rows : <SkeletonRows />}</tbody>
                </table>
              </div>
            </Card>
          )}
        </PageBody>
      }
    />
  );
}

function SkeletonRows() {
  return Array.from({ length: 8 }, (_, i) => (
    // biome-ignore lint/suspicious/noArrayIndexKey: static placeholder rows
    <tr key={i}>
      <td colSpan={4} className={cn(CELL, "px-4 py-2.5")}>
        <Skeleton className="h-3.5 w-full max-w-md" />
      </td>
    </tr>
  ));
}

function Chevron({ open }: { open: boolean }) {
  return (
    <ChevronRight
      aria-hidden
      className={cn(
        "size-4 shrink-0 text-muted-foreground motion-safe:transition-transform motion-safe:duration-200",
        open && "rotate-90",
      )}
    />
  );
}

function DeviceRow({
  icon: Icon,
  name,
  count,
  open,
  onToggle,
}: {
  icon: LucideIcon;
  name: string;
  count: number;
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <tr className="bg-foreground/[0.04]">
      <td colSpan={4} className={cn(CELL, "p-0")}>
        <button
          type="button"
          aria-expanded={open}
          onClick={onToggle}
          className="press flex w-full items-center gap-2.5 px-4 py-2 text-left transition-colors duration-200 hover:bg-foreground/[0.04]"
        >
          <Chevron open={open} />
          <span className="flex size-6 shrink-0 items-center justify-center rounded-md bg-primary/12 text-primary">
            <Icon className="size-3.5" />
          </span>
          <span className="min-w-0 flex-1 truncate font-display text-[13px] font-semibold tracking-wide">
            {name}
          </span>
          <span className="num text-[11px] text-muted-foreground">{count}</span>
        </button>
      </td>
    </tr>
  );
}

function GroupRow({
  name,
  count,
  open,
  onToggle,
}: {
  name: string;
  count: number;
  open: boolean;
  onToggle: () => void;
}) {
  return (
    <tr>
      <td colSpan={4} className={cn(CELL, "p-0")}>
        <button
          type="button"
          aria-expanded={open}
          onClick={onToggle}
          className="press flex w-full items-center gap-2 py-1.5 pr-4 pl-9 text-left text-[10px] font-semibold uppercase tracking-wider text-muted-foreground transition-colors duration-200 hover:text-foreground"
        >
          <Chevron open={open} />
          <span className="truncate">{name}</span>
          <span className="num font-normal normal-case">{count}</span>
        </button>
      </td>
    </tr>
  );
}

function HintRow({ text }: { text: string }) {
  return (
    <tr>
      <td colSpan={4} className={cn(CELL, "py-2 pr-4 pl-16 text-xs text-muted-foreground")}>
        <span className="flex items-start gap-2">
          <Thermometer aria-hidden className="mt-0.5 size-3.5 shrink-0" />
          {text}
        </span>
      </td>
    </tr>
  );
}

function SensorRow({
  sensor,
  name,
  stat,
  locale,
}: {
  sensor: HwSensor;
  name: string;
  stat: Stat | undefined;
  locale: string;
}) {
  const text = formatSensor(sensor.value, sensor.unit, locale);
  const ref = useRef<HTMLSpanElement>(null);
  useFlash(ref, text);
  const pos = position(sensor.value, sensor.unit, stat);
  return (
    <tr className="transition-colors duration-150 hover:bg-foreground/[0.035]">
      <td className={cn(CELL, "truncate py-1.5 pr-3 pl-16")} title={name}>
        {name}
      </td>
      <td className={cn(CELL, "px-3 py-1")}>
        <div className="flex items-center justify-end gap-2.5">
          <span
            aria-hidden
            className={cn(
              "relative h-[3px] w-12 shrink-0 rounded-full bg-foreground/10",
              pos === null && "opacity-0",
            )}
          >
            {sensor.unit === "%" ? (
              <span
                className="absolute inset-y-0 left-0 rounded-full bg-primary/45 motion-safe:transition-[width] motion-safe:duration-700 motion-safe:ease-linear"
                style={{ width: `${(pos ?? 0) * 100}%` }}
              />
            ) : null}
            <span
              className="absolute top-1/2 h-2 w-0.5 -translate-y-1/2 rounded-full bg-primary motion-safe:transition-[left] motion-safe:duration-700 motion-safe:ease-linear"
              style={{ left: `calc(${(pos ?? 0) * 100}% - 1px)` }}
            />
          </span>
          <span ref={ref} className="num rounded px-1.5 py-0.5 font-semibold">
            {text}
          </span>
        </div>
      </td>
      <td className={cn(CELL, "num px-3 py-1.5 text-right text-xs text-muted-foreground")}>
        {stat ? formatSensor(stat.min, sensor.unit, locale) : "—"}
      </td>
      <td className={cn(CELL, "num px-3 py-1.5 text-right text-xs text-muted-foreground")}>
        {stat ? formatSensor(stat.max, sensor.unit, locale) : "—"}
      </td>
    </tr>
  );
}
