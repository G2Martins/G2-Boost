import { useEffect, useId, useState } from "react";
import { CountUp } from "@/components/ui/count-up";
import { cn } from "@/lib/utils";

/**
 * 270° arc gauge with a gradient stroke and a needle. `value`/`max` drive both the arc fill and
 * the needle; colors come from tokens. Needle (CSS rotate) and arc (dash length) glide together
 * to each new value (`.gauge-anim`, off under reduced motion) and sweep up from zero on mount;
 * pass `format` to tween the caption number with the value (otherwise `valueText` is shown as is).
 */
export function Gauge({
  value,
  max,
  label,
  valueText,
  format,
  tone,
  className,
}: {
  value: number;
  max: number;
  label: string;
  valueText: string;
  format?: (n: number) => string;
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
  const target = max > 0 ? Math.min(1, Math.max(0, value / max)) : 0;
  // First frame at zero so the arc and needle sweep up to the first value.
  const [ready, setReady] = useState(false);
  useEffect(() => {
    const raf = requestAnimationFrame(() => setReady(true));
    return () => cancelAnimationFrame(raf);
  }, []);
  const ratio = ready ? target : 0;

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
        {/* Always mounted: the filled part is a dash of the full arc, so it can glide (a
            changing `d` cannot) and never re-plays an entrance when the value nears zero. */}
        <path
          d={arc(startAngle, startAngle + sweep)}
          pathLength={1}
          className="gauge-anim"
          fill="none"
          stroke={`url(#${id})`}
          strokeWidth={stroke}
          strokeLinecap="round"
          style={{ strokeDasharray: `${ratio} 1`, opacity: ratio > 0.005 ? 1 : 0 }}
        />
        {/* Needle drawn pointing right and rotated: `transform` animates, x2/y2 do not. */}
        <line
          x1={cx}
          y1={cy}
          x2={cx + r * 0.62}
          y2={cy}
          className="gauge-anim"
          stroke="hsl(var(--foreground) / 0.85)"
          strokeWidth={4}
          strokeLinecap="round"
          style={{ transform: `rotate(${needleAngle}deg)`, transformOrigin: `${cx}px ${cy}px` }}
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
        <div className="num font-display text-2xl font-semibold">
          {format ? <CountUp value={value} format={format} /> : valueText}
        </div>
        <div className="text-xs text-muted-foreground">{label}</div>
      </figcaption>
    </figure>
  );
}
