/**
 * Monitor motion test math, no DOM: the page records the time between animation frames
 * (ms) and this turns them into a refresh rate, missed frames and pixels per frame.
 */

/** A frame this many times the usual one missed its slot. */
export const LATE_FACTOR = 1.5;
/** A frame this many times the usual one is a pause (hidden window, moved window), not a drop. */
export const PAUSE_FACTOR = 10;

export type RefreshEstimate = {
  /** Measured refresh rate. */
  hz: number | null;
  /** The usual frame time (median). */
  frameMs: number | null;
  /** Frames that missed their slot (a 3x frame misses two). */
  dropped: number;
  /** Frame times that counted (pauses left out). */
  frames: number;
  /** The slowest frame that counted. */
  worstMs: number | null;
};

const median = (xs: number[]) => {
  const s = [...xs].sort((a, b) => a - b);
  const mid = s.length / 2;
  return s.length % 2
    ? (s[Math.floor(mid)] as number)
    : ((s[mid - 1] as number) + (s[mid] as number)) / 2;
};

/** Frames missed behind one frame time: 0 when on time, never for a pause. */
export function missedFrames(deltaMs: number, usualMs: number): number {
  if (!(deltaMs > 0) || !(usualMs > 0) || deltaMs > PAUSE_FACTOR * usualMs) return 0;
  return deltaMs > LATE_FACTOR * usualMs ? Math.max(1, Math.round(deltaMs / usualMs) - 1) : 0;
}

/**
 * Refresh rate from frame times. The usual frame is the median; the rate is the mean of the
 * on-time frames (so a few drops do not drag it down); pauses and garbage are ignored.
 */
export function estimateRefresh(deltas: number[]): RefreshEstimate {
  const valid = deltas.filter((d) => Number.isFinite(d) && d > 0);
  if (!valid.length) return { hz: null, frameMs: null, dropped: 0, frames: 0, worstMs: null };
  const usual = median(valid);
  const counted = valid.filter((d) => d <= PAUSE_FACTOR * usual);
  const onTime = counted.filter((d) => d <= LATE_FACTOR * usual);
  const frameAvg = onTime.reduce((a, b) => a + b, 0) / onTime.length;
  return {
    hz: 1000 / frameAvg,
    frameMs: usual,
    dropped: counted.reduce((n, d) => n + missedFrames(d, usual), 0),
    frames: counted.length,
    worstMs: Math.max(...counted),
  };
}

/** Rates monitors are sold with. */
const STANDARD_HZ = [
  24, 30, 48, 50, 60, 75, 90, 100, 120, 144, 165, 180, 200, 240, 280, 300, 360, 480, 500,
];

/** The standard rate a measurement is within 1.5% of (59.94 reads as 60), or null. */
export function nearestStandardHz(hz: number | null): number | null {
  if (hz === null || !Number.isFinite(hz) || hz <= 0) return null;
  const best = STANDARD_HZ.reduce((a, b) => (Math.abs(b - hz) < Math.abs(a - hz) ? b : a));
  return Math.abs(best - hz) / best <= 0.015 ? best : null;
}

/** Pixels an object moves per frame at `speed` px/s on a display refreshing at `hz`. */
export function pixelsPerFrame(speed: number, hz: number | null): number | null {
  return hz !== null && hz > 0 && Number.isFinite(speed) ? speed / hz : null;
}
