import { useEffect, useRef, useState } from "react";
import { type I18nKey, useI18n } from "@/lib/i18n";
import {
  estimateRefresh,
  missedFrames,
  nearestStandardHz,
  pixelsPerFrame,
  type RefreshEstimate,
} from "@/lib/refresh";

/*
 * TEST SIGNALS, NOT INTERFACE. A monitor test has to put exact levels on screen, and the design
 * tokens are tinted greys, so every colour in this file is generated from numbers (0 to 255)
 * instead of coming from a token. The page around the patterns (tiles, hint, readouts) is
 * ordinary interface and uses tokens only.
 */
const rgb = (r: number, g: number, b: number) => `rgb(${r}, ${g}, ${b})`;
const gray = (n: number) => rgb(n, n, n);

export type GroupId =
  | "solid"
  | "blacks"
  | "whites"
  | "gradient"
  | "uniformity"
  | "sharpness"
  | "motion";

/** One screen of a test: its name (shown in the hint) and, for flat fills, the colour. */
export type Step = { name: I18nKey; fill?: string };

export const GROUPS: { id: GroupId; title: I18nKey; desc: I18nKey; steps: Step[] }[] = [
  {
    id: "solid",
    title: "tools.monitor.t.solid",
    desc: "tools.monitor.t.solid.desc",
    steps: [
      { name: "tools.monitor.c.black", fill: gray(0) },
      { name: "tools.monitor.c.white", fill: gray(255) },
      { name: "tools.monitor.c.red", fill: rgb(255, 0, 0) },
      { name: "tools.monitor.c.green", fill: rgb(0, 255, 0) },
      { name: "tools.monitor.c.blue", fill: rgb(0, 0, 255) },
    ],
  },
  {
    id: "blacks",
    title: "tools.monitor.t.blacks",
    desc: "tools.monitor.t.blacks.desc",
    steps: [{ name: "tools.monitor.t.blacks" }],
  },
  {
    id: "whites",
    title: "tools.monitor.t.whites",
    desc: "tools.monitor.t.whites.desc",
    steps: [{ name: "tools.monitor.t.whites" }],
  },
  {
    id: "gradient",
    title: "tools.monitor.t.gradient",
    desc: "tools.monitor.t.gradient.desc",
    steps: [{ name: "tools.monitor.t.gradient" }],
  },
  {
    id: "uniformity",
    title: "tools.monitor.t.uniformity",
    desc: "tools.monitor.t.uniformity.desc",
    steps: [
      { name: "tools.monitor.c.black", fill: gray(0) },
      { name: "tools.monitor.u.dark", fill: gray(24) },
      { name: "tools.monitor.u.mid", fill: gray(128) },
    ],
  },
  {
    id: "sharpness",
    title: "tools.monitor.t.sharpness",
    desc: "tools.monitor.t.sharpness.desc",
    steps: [{ name: "tools.monitor.t.sharpness" }],
  },
  {
    id: "motion",
    title: "tools.monitor.t.motion",
    desc: "tools.monitor.t.motion.desc",
    steps: [{ name: "tools.monitor.t.motion" }],
  },
];

/** Hard-edged stripes of the given colours, for the small previews on the tiles. */
const stripes = (colors: string[]) =>
  `linear-gradient(90deg, ${colors
    .map((c, i) => `${c} ${(100 * i) / colors.length}% ${(100 * (i + 1)) / colors.length}%`)
    .join(", ")})`;

const range = (from: number, to: number) =>
  Array.from({ length: to - from + 1 }, (_, i) => from + i);

export const PREVIEW: Record<GroupId, string> = {
  solid: stripes([gray(0), gray(255), rgb(255, 0, 0), rgb(0, 255, 0), rgb(0, 0, 255)]),
  blacks: stripes(range(0, 10).map(gray)),
  whites: stripes(range(245, 255).map(gray)),
  gradient: `linear-gradient(90deg, ${gray(0)}, ${gray(255)})`,
  uniformity: stripes([gray(0), gray(24), gray(128)]),
  sharpness: `repeating-conic-gradient(${gray(0)} 0% 25%, ${gray(255)} 0% 50%) 0 0 / 8px 8px`,
  motion: stripes([gray(48), gray(48), gray(48), gray(235), gray(48), gray(48)]),
};

/* ---------------- flat fills and level squares ---------------- */

/** Squares of `levels` on a flat ground: which ones can you still tell from the ground? */
function LevelSquares({ levels, ground }: { levels: number[]; ground: number }) {
  return (
    <div
      className="flex size-full items-center justify-center"
      style={{ background: gray(ground) }}
    >
      <div className="flex gap-[1.8vw]">
        {levels.map((n) => (
          <div key={n} className="flex flex-col items-center gap-[1.4vw]">
            <div style={{ width: "6.6vw", height: "6.6vw", background: gray(n) }} />
            <span className="num text-[1.1vw]" style={{ color: gray(110) }}>
              {n}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

/* ---------------- canvas patterns, drawn in physical pixels ---------------- */

type Draw = (ctx: CanvasRenderingContext2D, w: number, h: number, dpr: number) => void;

/** Four ramps (grey, red, green, blue) of 256 discrete levels, so banding is the monitor's, not the browser's. */
const drawRamps: Draw = (ctx, w, h) => {
  const bands = [
    (v: number) => rgb(v, v, v),
    (v: number) => rgb(v, 0, 0),
    (v: number) => rgb(0, v, 0),
    (v: number) => rgb(0, 0, v),
  ];
  const bandH = h / bands.length;
  bands.forEach((color, b) => {
    for (let v = 0; v < 256; v++) {
      const x0 = Math.floor((v * w) / 256);
      const x1 = Math.floor(((v + 1) * w) / 256);
      ctx.fillStyle = color(v);
      ctx.fillRect(x0, Math.round(b * bandH), x1 - x0, Math.ceil(bandH));
    }
  });
};

/**
 * Pixel grid: a 1 px checkerboard, 1 px vertical and horizontal lines, small text, a circle and a
 * frame on the very last pixels. Drawn 1:1 in physical pixels, so any blur, colour fringe,
 * crop (overscan) or oval circle is the monitor's.
 */
const drawSharpness: Draw = (ctx, w, h, dpr) => {
  ctx.fillStyle = gray(0);
  ctx.fillRect(0, 0, w, h);
  const qw = Math.floor(w / 2);
  const qh = Math.floor(h / 2);

  const checker = ctx.createImageData(qw, qh);
  for (let y = 0; y < qh; y++) {
    for (let x = 0; x < qw; x++) {
      const i = (y * qw + x) * 4;
      const v = (x + y) % 2 ? 255 : 0;
      checker.data[i] = checker.data[i + 1] = checker.data[i + 2] = v;
      checker.data[i + 3] = 255;
    }
  }
  ctx.putImageData(checker, 0, 0);

  ctx.fillStyle = gray(255);
  for (let x = qw; x < w; x += 2) ctx.fillRect(x, 0, 1, qh); // vertical lines, top right
  for (let y = qh; y < h; y += 2) ctx.fillRect(0, y, qw, 1); // horizontal lines, bottom left

  // Bottom right: text, white on black above and black on white below, from small to large.
  const sizes = [9, 11, 13, 16, 20, 28].map((s) => Math.round(s * dpr));
  const pad = Math.round(14 * dpr);
  ctx.textBaseline = "top";
  let y = qh + pad;
  const half = Math.floor((h - qh - pad) / 2);
  for (const dark of [false, true]) {
    if (dark) {
      ctx.fillStyle = gray(255);
      ctx.fillRect(qw, qh + half + Math.round(pad / 2), w - qw, h);
      y = qh + half + pad;
    }
    ctx.fillStyle = dark ? gray(0) : gray(255);
    for (const size of sizes) {
      ctx.font = `${size}px "Segoe UI", system-ui, sans-serif`;
      ctx.fillText("Hamburgefonstiv 0123456789 iIl1 oO0", qw + pad, y);
      y += Math.round(size * 1.35);
    }
  }

  // Circle (must be round) and the frame (must show whole on all four sides).
  ctx.strokeStyle = gray(160);
  ctx.lineWidth = 1;
  ctx.beginPath();
  ctx.arc(w / 2, h / 2, Math.min(w, h) / 4, 0, Math.PI * 2);
  ctx.stroke();
  ctx.strokeStyle = gray(255);
  ctx.strokeRect(0.5, 0.5, w - 1, h - 1);
};

function Canvas({ draw }: { draw: Draw }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const paint = () => {
      const dpr = window.devicePixelRatio || 1;
      const box = canvas.getBoundingClientRect();
      canvas.width = Math.max(1, Math.round(box.width * dpr));
      canvas.height = Math.max(1, Math.round(box.height * dpr));
      const ctx = canvas.getContext("2d");
      if (ctx) draw(ctx, canvas.width, canvas.height, dpr);
    };
    paint();
    const watch = new ResizeObserver(paint);
    watch.observe(canvas);
    return () => watch.disconnect();
  }, [draw]);
  return <canvas ref={ref} className="block size-full" aria-hidden />;
}

/* ---------------- motion ---------------- */

/** Speeds in physical pixels per second: from easy to follow to a blur if the panel is slow. */
const LANES = [240, 480, 960, 1920] as const;

export type MotionResult = { hz: number; dropped: number; seconds: number };

type Live = RefreshEstimate & { droppedTotal: number; seconds: number };

/**
 * Squares crossing the screen at four speeds, drawn from the clock (not from a frame count) so a
 * dropped frame is a visible jump and not a slower square. The refresh rate is measured from the
 * time between animation frames. This animates on purpose, whatever the reduced motion setting
 * says: it only exists while the user runs it, and the launcher page says so.
 */
function MotionTest({ onResult }: { onResult: (r: MotionResult) => void }) {
  const { t, locale } = useI18n();
  const ref = useRef<HTMLCanvasElement>(null);
  const [live, setLive] = useState<Live | null>(null);

  useEffect(() => {
    const canvas = ref.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    const dpr = window.devicePixelRatio || 1;
    const size = Math.round(40 * dpr);
    const fit = () => {
      const box = canvas.getBoundingClientRect();
      canvas.width = Math.max(1, Math.round(box.width * dpr));
      canvas.height = Math.max(1, Math.round(box.height * dpr));
    };
    fit();
    const watch = new ResizeObserver(fit);
    watch.observe(canvas);

    const deltas: number[] = [];
    let usual: number | null = null;
    let dropped = 0;
    let frames = 0;
    let previous = 0;
    let start = 0;
    let published = 0;
    let latest: Live | null = null;
    let raf = 0;
    const loop = (now: number) => {
      if (!start) start = now;
      if (previous) {
        const delta = now - previous;
        deltas.push(delta);
        if (deltas.length > 240) deltas.shift();
        frames++;
        if (usual !== null) dropped += missedFrames(delta, usual);
      }
      previous = now;

      const { width, height } = canvas;
      ctx.fillStyle = gray(48);
      ctx.fillRect(0, 0, width, height);
      ctx.fillStyle = gray(235);
      LANES.forEach((speed, i) => {
        const x = ((((now - start) / 1000) * speed) % (width + size)) - size;
        ctx.fillRect(x, ((i + 0.5) * height) / LANES.length - size / 2, size, size);
      });

      if (now - published > 250) {
        published = now;
        const est = estimateRefresh(deltas);
        if (deltas.length >= 30) usual = est.frameMs;
        latest = { ...est, droppedTotal: dropped, seconds: (now - start) / 1000 };
        setLive(latest);
      }
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(raf);
      watch.disconnect();
      if (latest?.hz && frames >= 30) {
        onResult({ hz: latest.hz, dropped: latest.droppedTotal, seconds: latest.seconds });
      }
    };
  }, [onResult]);

  const one = new Intl.NumberFormat(locale, { maximumFractionDigits: 1 });
  const two = new Intl.NumberFormat(locale, { maximumFractionDigits: 2 });
  const standard = nearestStandardHz(live?.hz ?? null);
  return (
    <div className="flex size-full flex-col bg-background text-foreground">
      <div className="flex flex-wrap items-end gap-x-10 gap-y-2 border-b border-border px-6 py-3">
        <div>
          <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
            {t("tools.monitor.motion.rate")}
          </p>
          <p data-testid="motion-rate" className="num font-display text-3xl font-semibold">
            {live?.hz ? one.format(live.hz) : t("tools.monitor.motion.waiting")}
            {live?.hz ? <span className="ml-1 text-base font-normal">Hz</span> : null}
          </p>
          {standard ? (
            <p className="text-xs text-muted-foreground">
              {t("tools.monitor.motion.standard", { hz: standard })}
            </p>
          ) : null}
        </div>
        <Readout
          label={t("tools.monitor.motion.frame")}
          value={live?.frameMs}
          unit="ms"
          fmt={two}
        />
        <Readout
          label={t("tools.monitor.motion.dropped")}
          value={live ? live.droppedTotal : null}
          fmt={one}
        />
        <Readout
          label={t("tools.monitor.motion.worst")}
          value={live?.worstMs}
          unit="ms"
          fmt={two}
        />
        <p className="ml-auto max-w-md text-xs text-muted-foreground">
          {t("tools.monitor.motion.help")}
        </p>
      </div>
      <div className="grid min-h-0 flex-1 grid-cols-[13rem_1fr]">
        <div className="grid grid-rows-4 border-r border-border">
          {LANES.map((speed) => {
            const ppf = pixelsPerFrame(speed, live?.hz ?? null);
            return (
              <div key={speed} className="flex flex-col justify-center px-5">
                <span className="num text-sm font-semibold">
                  {t("tools.monitor.motion.lane", { speed })}
                </span>
                <span className="num text-xs text-muted-foreground">
                  {ppf === null ? "—" : t("tools.monitor.motion.ppf", { px: one.format(ppf) })}
                </span>
              </div>
            );
          })}
        </div>
        <canvas ref={ref} className="block size-full" aria-hidden />
      </div>
    </div>
  );
}

function Readout({
  label,
  value,
  unit,
  fmt,
}: {
  label: string;
  value: number | null | undefined;
  unit?: string;
  fmt: Intl.NumberFormat;
}) {
  return (
    <div>
      <p className="text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
        {label}
      </p>
      <p className="num font-display text-xl font-semibold">
        {value === null || value === undefined ? "—" : fmt.format(value)}
        {unit && value !== null && value !== undefined ? (
          <span className="ml-1 text-xs font-normal text-muted-foreground">{unit}</span>
        ) : null}
      </p>
    </div>
  );
}

/* ---------------- the pattern for a step ---------------- */

export function Pattern({
  group,
  step,
  onMotion,
}: {
  group: GroupId;
  step: number;
  onMotion: (r: MotionResult) => void;
}) {
  const fill = GROUPS.find((g) => g.id === group)?.steps[step]?.fill;
  switch (group) {
    case "solid":
    case "uniformity":
      return <div className="size-full" style={{ background: fill }} />;
    case "blacks":
      return <LevelSquares levels={range(0, 10)} ground={0} />;
    case "whites":
      return <LevelSquares levels={range(245, 255)} ground={255} />;
    case "gradient":
      return <Canvas draw={drawRamps} />;
    case "sharpness":
      return <Canvas draw={drawSharpness} />;
    case "motion":
      return <MotionTest onResult={onMotion} />;
  }
}
