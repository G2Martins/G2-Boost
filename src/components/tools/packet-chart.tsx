import { type Cell, LATE_MS } from "@/lib/pktloss";

function niceCeil(v: number): number {
  const mag = 10 ** Math.floor(Math.log10(v));
  const n = v / mag;
  return (n <= 1 ? 1 : n <= 2 ? 2 : n <= 5 ? 5 : 10) * mag;
}

/**
 * Response time of every packet, left to right in the order they were sent. Lost packets are a
 * red band from top to bottom (the line breaks there), late ones a dot on the line.
 */
export function PacketChart({
  cells,
  label,
  limitLabel,
}: {
  cells: (Cell | undefined)[];
  label: string;
  limitLabel: string;
}) {
  const W = 800;
  const H = 200;
  const padL = 40;
  const padR = 8;
  const padT = 8;
  const padB = 14;
  const total = Math.max(1, cells.length);
  const slot = (W - padL - padR) / total;
  const peak = cells.reduce((m, c) => (c && c.rtt !== null ? Math.max(m, c.rtt) : m), 30);
  const yMax = niceCeil(peak * 1.15);
  const x = (i: number) => padL + (i + 0.5) * slot;
  const y = (v: number) => padT + (1 - Math.min(v, yMax) / yMax) * (H - padT - padB);

  let line = "";
  let pen = false;
  cells.forEach((c, i) => {
    if (!c || c.rtt === null) {
      pen = false;
      return;
    }
    line += `${pen ? "L" : "M"}${x(i).toFixed(1)},${y(c.rtt).toFixed(1)} `;
    pen = true;
  });
  const marks = cells.flatMap((c, i) => (c ? [{ i, c }] : []));

  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-52 w-full" role="img" aria-label={label}>
      {[0, yMax / 2, yMax].map((v) => (
        <g key={v}>
          <line
            x1={padL}
            x2={W - padR}
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
            {Math.round(v)}
          </text>
        </g>
      ))}
      {yMax >= LATE_MS ? (
        <g>
          <line
            x1={padL}
            x2={W - padR}
            y1={y(LATE_MS)}
            y2={y(LATE_MS)}
            stroke="hsl(var(--warning) / 0.6)"
            strokeDasharray="5 4"
          />
          <text
            x={W - padR - 4}
            y={y(LATE_MS) - 4}
            textAnchor="end"
            fontSize="10"
            fill="hsl(var(--warning))"
            className="num"
          >
            {limitLabel}
          </text>
        </g>
      ) : null}
      {marks.map(({ i, c }) =>
        c.rtt === null ? (
          <rect
            key={i}
            x={x(i) - Math.max(1.5, slot) / 2}
            y={padT}
            width={Math.max(1.5, slot)}
            height={H - padT - padB}
            fill="hsl(var(--destructive) / 0.4)"
          />
        ) : null,
      )}
      <path
        d={line}
        fill="none"
        stroke="hsl(var(--chart-1))"
        strokeWidth={1.6}
        strokeLinejoin="round"
        strokeLinecap="round"
      />
      {marks.map(({ i, c }) =>
        c.rtt !== null && c.late ? (
          <circle key={i} cx={x(i)} cy={y(c.rtt)} r={3} fill="hsl(var(--warning))" />
        ) : null,
      )}
    </svg>
  );
}
