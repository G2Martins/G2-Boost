import { type CSSProperties, useId } from "react";

/**
 * Rolling line chart for live samples (last `window` points). Pure SVG, token colours,
 * no library. `max` fixes the Y range (e.g. 100 for percentages); omit it for auto-scale.
 * With `seq` (a counter that grows by one per sample) the data scrolls continuously: each new
 * sample re-mounts the plot one step to the right and it slides back over the 1 s tick
 * (`.live-scroll`, off under reduced motion). Without `seq` the lines draw in once.
 */
export function LiveLine({
  series,
  window: win,
  max,
  format,
  seq,
  className,
  label,
}: {
  series: { values: (number | null)[]; color: string }[];
  window: number;
  max?: number;
  /** Tick label formatter (default: integer). */
  format?: (v: number) => string;
  /** Sample counter for continuous scrolling (live monitors). */
  seq?: number;
  className?: string;
  label: string;
}) {
  const gid = useId();
  const W = 600;
  const H = 180;
  const padL = 44;
  const padB = 14;
  const fmt = format ?? ((v: number) => Math.round(v).toString());
  const peak = Math.max(
    1,
    ...series.flatMap((s) => s.values.filter((v): v is number => v !== null)),
  );
  const yMax = max ?? niceCeil(peak * 1.15);
  const step = (W - padL - 8) / Math.max(1, win - 1);
  const x = (i: number) => padL + i * step;
  const y = (v: number) => 8 + (1 - Math.min(v, yMax) / yMax) * (H - padB - 8);
  const path = (values: (number | null)[]) => {
    let d = "";
    let pen = false;
    const offset = win - values.length;
    values.forEach((v, i) => {
      if (v === null) {
        pen = false;
        return;
      }
      d += `${pen ? "L" : "M"}${x(i + offset).toFixed(1)},${y(v).toFixed(1)} `;
      pen = true;
    });
    return d;
  };
  const ticks = [0, yMax / 2, yMax];
  const first = series[0];
  // Area under the first series: its last contiguous run, closed to the baseline.
  const area = (() => {
    if (!first) return "";
    const offset = win - first.values.length;
    let end = first.values.length - 1;
    while (end >= 0 && first.values[end] === null) end--;
    let start = end;
    while (start > 0 && first.values[start - 1] !== null) start--;
    if (end - start < 1) return "";
    let d = `M${x(start + offset).toFixed(1)},${y(0).toFixed(1)} `;
    for (let i = start; i <= end; i++)
      d += `L${x(i + offset).toFixed(1)},${y(first.values[i] ?? 0).toFixed(1)} `;
    return `${d}L${x(end + offset).toFixed(1)},${y(0).toFixed(1)} Z`;
  })();
  const live = seq !== undefined;
  const hasData = series.some((s) => s.values.filter((v) => v !== null).length > 1);
  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      className={className ?? "h-44 w-full"}
      role="img"
      aria-label={label}
    >
      <defs>
        {first ? (
          <linearGradient id={gid} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor={first.color} stopOpacity={0.16} />
            <stop offset="100%" stopColor={first.color} stopOpacity={0} />
          </linearGradient>
        ) : null}
        {/* The scrolling plot must not slide over the Y labels or past the right edge. */}
        <clipPath id={`${gid}c`}>
          <rect x={padL} y={0} width={W - 8 - padL} height={H} />
        </clipPath>
      </defs>
      {ticks.map((v) => (
        <g key={v}>
          <line
            x1={padL}
            x2={W - 8}
            y1={y(v)}
            y2={y(v)}
            stroke="hsl(var(--border))"
            strokeDasharray="2 4"
          />
          <text
            x={padL - 6}
            y={y(v) + 4}
            textAnchor="end"
            fontSize="10"
            fill="hsl(var(--muted-foreground))"
            className="num"
          >
            {fmt(v)}
          </text>
        </g>
      ))}
      <g clipPath={`url(#${gid}c)`}>
        <g
          key={seq ?? "static"}
          className={live ? "live-scroll" : undefined}
          style={{ "--step": `${step}px` } as CSSProperties}
        >
          {area ? (
            <path d={area} fill={`url(#${gid})`} className={live ? undefined : "fade-late"} />
          ) : null}
          {series.map((s, idx) => (
            <path
              // biome-ignore lint/suspicious/noArrayIndexKey: series order is stable by construction
              key={idx}
              d={path(s.values)}
              pathLength={1}
              // Static charts draw in once on real data; live ones enter by scrolling.
              className={!live && hasData ? "draw-line" : undefined}
              fill="none"
              stroke={s.color}
              strokeWidth={1.8}
              strokeLinejoin="round"
              strokeLinecap="round"
            />
          ))}
        </g>
      </g>
    </svg>
  );
}

function niceCeil(v: number): number {
  if (v <= 0) return 1;
  const mag = 10 ** Math.floor(Math.log10(v));
  const n = v / mag;
  const step = n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10;
  return step * mag;
}
