/**
 * Rolling line chart for live samples (last `window` points). Pure SVG, token colours,
 * no library. `max` fixes the Y range (e.g. 100 for percentages); omit it for auto-scale.
 */
export function LiveLine({
  series,
  window: win,
  max,
  format,
  className,
  label,
}: {
  series: { values: (number | null)[]; color: string }[];
  window: number;
  max?: number;
  /** Tick label formatter (default: integer). */
  format?: (v: number) => string;
  className?: string;
  label: string;
}) {
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
  const x = (i: number) => padL + (i / Math.max(1, win - 1)) * (W - padL - 8);
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
  return (
    <svg
      viewBox={`0 0 ${W} ${H}`}
      className={className ?? "h-44 w-full"}
      role="img"
      aria-label={label}
    >
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
      {series.map((s, idx) => (
        <path
          // biome-ignore lint/suspicious/noArrayIndexKey: series order is stable by construction
          key={idx}
          d={path(s.values)}
          fill="none"
          stroke={s.color}
          strokeWidth={1.8}
          strokeLinejoin="round"
          strokeLinecap="round"
        />
      ))}
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
