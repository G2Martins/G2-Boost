/**
 * Mouse test math, no DOM: the page feeds in the timestamps of pointer reports (ms, from the
 * event) and the button presses, and reads the numbers back. Docs/core/tools.md.
 */

/** A silence longer than this is the hand pausing, not the mouse reporting (ms). */
export const GAP_MS = 50;
/** Recent intervals behind "now" and behind the stall check. */
export const WINDOW = 64;
/** Intervals needed before "now" and the stall check mean anything. */
const MIN_WINDOW = 8;
/** An interval this many times the recent median is a hiccup of the page, not the mouse. */
export const STALL_FACTOR = 3;
/** Stalls in a row that mean the rate really changed (a rate button on the mouse). */
const RESYNC = 5;
/** Intervals kept for the chart. */
export const CHART_POINTS = 200;

export type RateSnapshot = {
  /** Reports counted (every timestamp that could be ordered). */
  reports: number;
  hzNow: number | null;
  hzAvg: number | null;
  /** Highest `hzNow` seen once the window was half full (one tiny interval alone proves nothing). */
  hzMax: number | null;
  /** Mean time between reports (ms). */
  avgMs: number | null;
  /** Mean difference between consecutive intervals (ms). */
  jitterMs: number | null;
  /** Intervals left out as hiccups of the page. */
  stalls: number;
  /** Pauses between movements. */
  gaps: number;
  /** The last intervals, oldest first (for the chart). */
  recent: number[];
};

const mean = (xs: number[]) => xs.reduce((a, b) => a + b, 0) / xs.length;
const toHz = (ms: number | null) => (ms !== null && ms > 0 ? 1000 / ms : null);

function median(xs: number[]): number {
  const s = [...xs].sort((a, b) => a - b);
  const mid = s.length / 2;
  return s.length % 2
    ? (s[Math.floor(mid)] as number)
    : ((s[mid - 1] as number) + (s[mid] as number)) / 2;
}

/**
 * Polling rate from report timestamps. Outliers are left out so the number describes the mouse:
 * a pause (>= 50 ms) is not an interval; an interval over 3x the recent median is a stall of
 * the page (counted, not averaged); five stalls in a row mean the rate itself changed, so the
 * window starts over. Intervals of 0 are real (two reports sharing a timestamp), so a 4000 Hz
 * mouse on a coarse clock still averages right.
 */
export class RateMeter {
  private last: number | null = null;
  private window: number[] = [];
  private recent: number[] = [];
  private sum = 0;
  private count = 0;
  private jitterSum = 0;
  private jitterCount = 0;
  private prev: number | null = null;
  private run = 0;
  private maxHz: number | null = null;
  private reports = 0;
  private stalls = 0;
  private gaps = 0;

  reset() {
    this.last = this.prev = this.maxHz = null;
    this.window = [];
    this.recent = [];
    this.sum = this.count = this.jitterSum = this.jitterCount = 0;
    this.run = this.reports = this.stalls = this.gaps = 0;
  }

  push(t: number) {
    // Anything that cannot be put in order (NaN, going back in time) is ignored.
    if (!Number.isFinite(t) || (this.last !== null && t < this.last)) return;
    this.reports++;
    const last = this.last;
    this.last = t;
    if (last === null) return;
    const dt = t - last;
    if (dt >= GAP_MS) {
      this.gaps++;
      this.prev = null;
      this.run = 0;
      return;
    }
    const med = this.window.length >= MIN_WINDOW ? median(this.window) : null;
    if (med !== null && med > 0 && dt > STALL_FACTOR * med) {
      if (this.run + 1 < RESYNC) {
        this.run++;
        this.stalls++;
        this.prev = null;
        return;
      }
      this.window = []; // the rate changed: measure it from here
      this.prev = null;
    }
    this.run = 0;
    this.accept(dt);
  }

  private accept(dt: number) {
    this.sum += dt;
    this.count++;
    this.window.push(dt);
    if (this.window.length > WINDOW) this.window.shift();
    this.recent.push(dt);
    if (this.recent.length > CHART_POINTS) this.recent.shift();
    if (this.prev !== null) {
      this.jitterSum += Math.abs(dt - this.prev);
      this.jitterCount++;
    }
    this.prev = dt;
    if (this.window.length >= WINDOW / 2) {
      const hz = toHz(mean(this.window));
      if (hz !== null && (this.maxHz === null || hz > this.maxHz)) this.maxHz = hz;
    }
  }

  snapshot(): RateSnapshot {
    const avgMs = this.count ? this.sum / this.count : null;
    return {
      reports: this.reports,
      hzNow: toHz(this.window.length >= MIN_WINDOW ? mean(this.window) : null),
      hzAvg: toHz(avgMs),
      hzMax: this.maxHz,
      avgMs,
      jitterMs: this.jitterCount ? this.jitterSum / this.jitterCount : null,
      stalls: this.stalls,
      gaps: this.gaps,
      recent: [...this.recent],
    };
  }
}

/** Rates mice are sold with: the marks on the ruler. */
export const STANDARD_RATES = [125, 250, 500, 1000, 2000, 4000, 8000] as const;

/** Where a rate sits on a log ruler from 125 to 8000 Hz: 0 to 1 (clamped). */
export function rulerPosition(hz: number): number {
  if (!Number.isFinite(hz) || hz <= 0) return 0;
  const lo = STANDARD_RATES[0];
  const hi = STANDARD_RATES[STANDARD_RATES.length - 1] as number;
  return Math.min(1, Math.max(0, Math.log(hz / lo) / Math.log(hi / lo)));
}

/* ---------------- buttons ---------------- */

/** Two presses of the same button closer than this are a failing switch, not a person (ms). */
export const BOUNCE_MS = 80;

export function isBounce(
  prevDown: number | null | undefined,
  now: number,
  limit = BOUNCE_MS,
): boolean {
  return (
    prevDown != null &&
    Number.isFinite(prevDown) &&
    Number.isFinite(now) &&
    now >= prevDown &&
    now - prevDown < limit
  );
}

/** `MouseEvent.button` order: left, middle, right, back, forward. */
export const BUTTONS = ["left", "middle", "right", "back", "forward"] as const;
export type ButtonId = (typeof BUTTONS)[number];

export type ButtonStat = {
  downs: number;
  /** Presses that came less than `BOUNCE_MS` after the previous one. */
  bounces: number;
  lastDown: number | null;
  /** Gap of the latest bounce (ms). */
  gapMs: number | null;
};

export const emptyButtons = (): ButtonStat[] =>
  BUTTONS.map(() => ({ downs: 0, bounces: 0, lastDown: null, gapMs: null }));

/** Stats after `button` went down at `t` (immutable; unknown buttons or times change nothing). */
export function pressButton(stats: ButtonStat[], button: number, t: number): ButtonStat[] {
  if (!Number.isInteger(button) || button < 0 || button >= BUTTONS.length || !Number.isFinite(t)) {
    return stats;
  }
  return stats.map((s, i) => {
    if (i !== button) return s;
    const bounce = isBounce(s.lastDown, t);
    return {
      downs: s.downs + 1,
      bounces: s.bounces + (bounce ? 1 : 0),
      lastDown: t,
      gapMs: bounce && s.lastDown !== null ? t - s.lastDown : s.gapMs,
    };
  });
}
