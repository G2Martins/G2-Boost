import { ShieldAlert } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import type { OverlayFrame, OverlayMetric, OverlaySettings } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { formatBytes } from "@/lib/utils";

// Shared by the overlay window (src/overlay/main.tsx) and the preview on the overlay page.
// Spans only: the preview renders it inside a button. No motion: numbers just change.

/** Same as the core's `OverlaySettings::default()` (used in the browser, where there is no backend). */
export const OVERLAY_DEFAULTS: OverlaySettings = {
  enabled: false,
  hotkey: "F7",
  corner: "top_left",
  metrics: ["fps", "frame_time", "low1", "cpu_pct", "gpu_pct", "gpu_temp", "ram"],
  style: "bar",
  opacity: 70,
  size: 100,
};

/** Display groups, in the core's `overlay::METRICS` order. */
export const HUD_GROUPS: { id: string; label?: I18nKey; metrics: OverlayMetric[] }[] = [
  { id: "game", metrics: ["game"] },
  { id: "fps", metrics: ["fps", "frame_time", "low1"] },
  { id: "cpu", label: "overlay.hud.cpu", metrics: ["cpu_pct", "cpu_mhz", "cpu_temp"] },
  { id: "gpu", label: "overlay.hud.gpu", metrics: ["gpu_pct", "gpu_temp", "gpu_mhz"] },
  { id: "ram", label: "overlay.hud.ram", metrics: ["ram"] },
  { id: "vram", label: "overlay.hud.vram", metrics: ["vram"] },
];

function Val({
  v,
  unit,
  lead,
  accent,
  min,
}: {
  v: ReactNode;
  unit?: string;
  lead?: string;
  accent?: boolean;
  /** Minimum width in characters, so the plate does not twitch when a digit is added. */
  min?: number;
}) {
  return (
    <span className="inline-flex items-baseline gap-[0.22em] whitespace-nowrap">
      {lead ? (
        <span className="text-[0.74em] font-semibold text-muted-foreground">{lead}</span>
      ) : null}
      <span
        className={
          accent
            ? "num inline-block text-right text-[1.18em] font-semibold text-primary"
            : "num inline-block text-right font-semibold"
        }
        style={min ? { minWidth: `${min}ch` } : undefined}
      >
        {v}
      </span>
      {unit ? (
        <span className="text-[0.7em] font-semibold text-muted-foreground">{unit}</span>
      ) : null}
    </span>
  );
}

/** The overlay itself: a dark plate (opacity from the settings) with tabular numbers. */
export function Hud({ frame }: { frame: OverlayFrame }) {
  const { t, locale } = useI18n();
  const { config: c, sys, fps } = frame;
  const shown = new Set(c.metrics);
  const num = (v: number | null | undefined, digits = 0) =>
    v == null || !Number.isFinite(v)
      ? "—"
      : new Intl.NumberFormat(locale, {
          minimumFractionDigits: digits,
          maximumFractionDigits: digits,
        }).format(v);
  const admin = frame.fps_state === "needs_admin";

  const cell = (m: OverlayMetric): ReactNode => {
    switch (m) {
      case "game":
        return frame.game ? (
          <span className="max-w-[18em] truncate text-[0.86em] font-semibold text-foreground/85">
            {frame.game}
          </span>
        ) : null;
      case "fps":
        return (
          <span className="inline-flex items-baseline gap-[0.3em]">
            <Val v={num(fps?.fps)} unit={t("overlay.hud.fps")} accent={!!fps} min={3} />
            {admin ? (
              <ShieldAlert
                aria-label={t("overlay.hud.needs_admin")}
                className="size-[0.95em] self-center text-warning"
              />
            ) : null}
          </span>
        );
      case "frame_time":
        return <Val v={num(fps?.frame_ms, 1)} unit="ms" min={4} />;
      case "low1":
        return <Val lead={t("overlay.hud.low1")} v={num(fps?.low1_fps)} min={3} />;
      case "cpu_pct":
        return <Val v={num(sys?.cpu_pct)} unit="%" min={3} />;
      case "cpu_mhz":
        return <Val v={num(sys?.cpu_mhz ? sys.cpu_mhz / 1000 : null, 2)} unit="GHz" min={4} />;
      case "cpu_temp":
        return <Val v={num(sys?.cpu_temp_c)} unit="°C" min={2} />;
      case "gpu_pct":
        return <Val v={num(sys?.gpu_pct)} unit="%" min={3} />;
      case "gpu_temp":
        return <Val v={num(sys?.gpu_temp_c)} unit="°C" min={2} />;
      case "gpu_mhz":
        return <Val v={num(sys?.gpu_core_mhz)} unit="MHz" min={4} />;
      case "ram":
        return <Val v={sys ? formatBytes(sys.ram_used, locale) : "—"} />;
      case "vram":
        return <Val v={sys ? formatBytes(sys.gpu_mem_bytes, locale) : "—"} />;
    }
  };

  const list = c.style === "list";
  const groups = HUD_GROUPS.map((g) => ({
    ...g,
    cells: g.metrics.filter((m) => shown.has(m)).map((m) => [m, cell(m)] as const),
  })).filter((g) => g.cells.some(([, node]) => node !== null));

  return (
    <span
      data-style={c.style}
      className={[
        "hud inline-flex rounded-[0.45em] border border-foreground/10 px-[0.8em] py-[0.5em] font-sans leading-tight text-foreground",
        "bg-[hsl(var(--card)/var(--hud-alpha))] shadow-[inset_0_1px_0_hsl(var(--foreground)/0.06)] [text-shadow:0_1px_2px_hsl(0_0%_0%/0.9)]",
        list ? "flex-col gap-[0.28em]" : "items-center gap-[0.85em]",
      ].join(" ")}
      style={
        { "--hud-alpha": c.opacity / 100, fontSize: `${(12 * c.size) / 100}px` } as CSSProperties
      }
    >
      {groups.map((g, i) => (
        <span
          key={g.id}
          className={[
            "flex items-baseline gap-[0.6em]",
            list ? "justify-between" : "",
            !list && i > 0 ? "border-l border-foreground/12 pl-[0.85em]" : "",
          ].join(" ")}
        >
          {g.label ? (
            <span className="text-[0.74em] font-semibold tracking-wide text-muted-foreground">
              {t(g.label)}
            </span>
          ) : null}
          <span className="flex items-baseline gap-[0.6em]">
            {g.cells.map(([m, node]) => (
              <span key={m}>{node}</span>
            ))}
          </span>
        </span>
      ))}
    </span>
  );
}
