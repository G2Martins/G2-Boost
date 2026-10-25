import type { ButtonId } from "@/lib/mouse";

const LIT = "hsl(var(--primary))";
const DIM = "hsl(var(--foreground) / 0.07)";
const EDGE = "hsl(var(--border-strong))";

/**
 * Top view of a mouse. Each button lights up while it is held; the wheel shows which way it
 * turned. Colors are tokens, the fill change is instant (a test signal must not lag).
 */
export function MouseFigure({
  down,
  wheel,
}: {
  down: Record<ButtonId, boolean>;
  wheel: "up" | "down" | null;
}) {
  const fill = (id: ButtonId) => (down[id] ? LIT : DIM);
  return (
    <svg viewBox="0 0 140 204" className="h-60 w-auto" aria-hidden>
      <path
        d="M30 84 C30 34 50 10 70 10 C90 10 110 34 110 84 L110 142 C110 180 92 194 70 194 C48 194 30 180 30 142 Z"
        fill="hsl(var(--card))"
        stroke={EDGE}
        strokeWidth={1.5}
      />
      <path
        d="M31 84 C31 40 50 12 65 11 L65 84 Z"
        fill={fill("left")}
        stroke={EDGE}
        strokeWidth={1}
      />
      <path
        d="M109 84 C109 40 90 12 75 11 L75 84 Z"
        fill={fill("right")}
        stroke={EDGE}
        strokeWidth={1}
      />
      <rect
        x={66}
        y={26}
        width={8}
        height={26}
        rx={4}
        fill={fill("middle")}
        stroke={EDGE}
        strokeWidth={1}
      />
      <polygon
        points="66,21 70,16 74,21"
        fill={wheel === "up" ? LIT : DIM}
        stroke={EDGE}
        strokeWidth={0.75}
      />
      <polygon
        points="66,57 70,62 74,57"
        fill={wheel === "down" ? LIT : DIM}
        stroke={EDGE}
        strokeWidth={0.75}
      />
      <line x1={31} y1={84} x2={109} y2={84} stroke={EDGE} strokeWidth={1} />
      <rect
        x={20}
        y={96}
        width={10}
        height={22}
        rx={3}
        fill={fill("forward")}
        stroke={EDGE}
        strokeWidth={1}
      />
      <rect
        x={20}
        y={122}
        width={10}
        height={22}
        rx={3}
        fill={fill("back")}
        stroke={EDGE}
        strokeWidth={1}
      />
    </svg>
  );
}
