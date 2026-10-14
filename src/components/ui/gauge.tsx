import { useId } from "react";
import { cn } from "@/lib/utils";

/**
 * 270° arc gauge (ExitLag-style) with a gradient stroke and a needle.
 * `value`/`max` drive both the arc fill and the needle; colors come from tokens.
 */
export function Gauge({
  value,
  max,
  label,
  valueText,
  tone,
  className,
}: {
  value: number;
  max: number;
  label: string;
  valueText: string;
  tone: "download" | "upload";
  className?: string;
}) {
  const id = useId();
  const size = 220;
  const stroke = 16;
  const r = (size - stroke) / 2 - 6;
  const cx = size / 2;
  const cy = size / 2;
  const startAngle = 135; // degrees, clockwise from +x
  const sweep = 270;
  const ratio = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;

  const polar = (deg: number) => {
    const rad = (deg * Math.PI) / 180;
    return [cx + r * Math.cos(rad), cy + r * Math.sin(rad)] as const;
  };
  const arc = (from: number, to: number) => {
    const [x1, y1] = polar(from);
    const [x2, y2] = polar(to);
    const large = to - from > 180 ? 1 : 0;
    return `M ${x1} ${y1} A ${r} ${r} 0 ${large} 1 ${x2} ${y2}`;
  };
  const needleAngle = startAngle + sweep * ratio;
  const [nx, ny] = polar(needleAngle);
  const start = `hsl(var(--gauge-${tone}-start))`;
  const end = `hsl(var(--gauge-${tone}-end))`;

  return (
    <figure className={cn("flex flex-col items-center", className)}>
      <svg
        width={size}
        height={size * 0.82}
        viewBox={`0 0 ${size} ${size * 0.82}`}
        role="img"
        aria-label={`${label} ${valueText}`}
      >
        <defs>
          <linearGradient id={id} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0%" stopColor={start} />
            <stop offset="100%" stopColor={end} />
          </linearGradient>
        </defs>
        <path
          d={arc(startAngle, startAngle + sweep)}
          fill="none"
          stroke="hsl(var(--border))"
          strokeWidth={stroke}
          strokeLinecap="round"
        />
        {ratio > 0.005 ? (
          <path
            d={arc(startAngle, needleAngle)}
            fill="none"
            stroke={`url(#${id})`}
            strokeWidth={stroke}
            strokeLinecap="round"
            style={{ transition: "d 0.4s ease-out" }}
          />
        ) : null}
        <line
          x1={cx}
          y1={cy}
          x2={cx + (nx - cx) * 0.62}
          y2={cy + (ny - cy) * 0.62}
          stroke="hsl(var(--foreground) / 0.85)"
          strokeWidth={4}
          strokeLinecap="round"
          style={{ transition: "all 0.4s ease-out" }}
        />
        <circle
          cx={cx}
          cy={cy}
          r={7}
          fill="hsl(var(--card))"
          stroke="hsl(var(--foreground) / 0.5)"
          strokeWidth={2}
        />
      </svg>
      <figcaption className="-mt-10 text-center">
        <div className="num font-display text-2xl font-semibold">{valueText}</div>
        <div className="text-xs text-muted-foreground">{label}</div>
      </figcaption>
    </figure>
  );
}
