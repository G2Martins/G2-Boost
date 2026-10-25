import { describe, expect, test } from "bun:test";
import {
  BOUNCE_MS,
  CHART_POINTS,
  emptyButtons,
  GAP_MS,
  isBounce,
  pressButton,
  RateMeter,
  rulerPosition,
  STANDARD_RATES,
  WINDOW,
} from "../src/lib/mouse";

/** `n` report timestamps `dt` ms apart. */
const steps = (n: number, dt: number, from = 0) => Array.from({ length: n }, (_, i) => from + i * dt);
const meter = (...runs: number[][]) => {
  const m = new RateMeter();
  for (const r of runs) for (const t of r) m.push(t);
  return m;
};
const near = (got: number | null, want: number, tol = 0.01) => {
  expect(got).not.toBeNull();
  expect(Math.abs((got as number) - want)).toBeLessThanOrEqual(tol);
};

describe("polling rate", () => {
  test("a steady 1000 Hz mouse reads 1000 Hz everywhere", () => {
    const s = meter(steps(500, 1)).snapshot();
    near(s.hzNow, 1000);
    near(s.hzAvg, 1000);
    near(s.hzMax, 1000);
    near(s.avgMs, 1);
    near(s.jitterMs, 0);
    expect(s.reports).toBe(500);
    expect(s.stalls).toBe(0);
    expect(s.gaps).toBe(0);
  });

  test("the usual rates come out of their intervals", () => {
    for (const hz of [125, 250, 500, 1000, 2000, 4000, 8000]) {
      near(meter(steps(300, 1000 / hz)).snapshot().hzAvg, hz, hz * 0.001);
    }
  });

  test("a pause between movements is not an interval", () => {
    const s = meter(steps(100, 1), steps(100, 1, 2099)).snapshot();
    near(s.hzAvg, 1000);
    expect(s.gaps).toBe(1);
    expect(s.reports).toBe(200);
    // Exactly at the limit still counts as a pause; just under it is a (slow) interval.
    expect(meter([0, GAP_MS]).snapshot().gaps).toBe(1);
    expect(meter([0, GAP_MS - 1]).snapshot().gaps).toBe(0);
  });

  test("a hiccup of the page is counted but not averaged", () => {
    const s = meter(steps(50, 1), steps(50, 1, 61)).snapshot();
    near(s.hzAvg, 1000);
    near(s.hzNow, 1000);
    expect(s.stalls).toBe(1);
    near(s.avgMs, 1);
    expect(s.recent).not.toContain(12);
  });

  test("the first intervals are accepted: there is nothing to compare them to yet", () => {
    const s = meter([0, 1, 2, 30]).snapshot();
    expect(s.stalls).toBe(0);
    expect(s.hzNow).toBeNull();
  });

  test("five slow intervals in a row mean the rate changed, not that the page stalled", () => {
    const fast = steps(100, 1);
    const slow = steps(30, 8, 99 + 8);
    const s = meter(fast, slow).snapshot();
    expect(s.stalls).toBe(4);
    near(s.hzNow, 125);
    expect(s.hzAvg).toBeGreaterThan(125);
    expect(s.hzAvg).toBeLessThan(1000);
  });

  test("reports that share a timestamp are real reports", () => {
    const doubled = steps(200, 1).flatMap((t) => [t, t]);
    near(meter(doubled).snapshot().hzAvg, 2000, 10);
  });

  test("garbage timestamps are ignored", () => {
    const m = new RateMeter();
    for (const t of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) m.push(t);
    expect(m.snapshot().reports).toBe(0);
    for (const t of steps(20, 1, 100)) m.push(t);
    m.push(50); // back in time
    m.push(Number.NaN);
    const s = m.snapshot();
    expect(s.reports).toBe(20);
    near(s.hzAvg, 1000);
    expect(new RateMeter().snapshot()).toMatchObject({ hzNow: null, hzAvg: null, hzMax: null, avgMs: null });
  });

  test("jitter is the mean change between neighbouring intervals", () => {
    const wobble = [0];
    for (let i = 1; i <= 100; i++) wobble.push((wobble[i - 1] as number) + (i % 2 ? 1 : 2));
    near(meter(wobble).snapshot().jitterMs, 1);
    near(meter(steps(100, 1)).snapshot().jitterMs, 0);
  });

  test("one tiny interval does not set the maximum", () => {
    const s = meter(steps(200, 1), [200.1], steps(200, 1, 201.1)).snapshot();
    expect(s.hzMax).toBeLessThan(1100);
    expect(s.hzMax).toBeGreaterThanOrEqual(1000 - 0.01);
  });

  test("the maximum only counts once the window is half full", () => {
    expect(meter(steps(WINDOW / 2 - 1, 0.1)).snapshot().hzMax).toBeNull();
    expect(meter(steps(WINDOW, 0.1)).snapshot().hzMax).not.toBeNull();
  });

  test("the chart keeps the latest intervals only", () => {
    const m = meter(steps(CHART_POINTS + 50, 1));
    expect(m.snapshot().recent).toHaveLength(CHART_POINTS);
    // The snapshot is a copy: drawing it cannot change the meter.
    m.snapshot().recent.fill(99);
    expect(m.snapshot().recent[0]).toBe(1);
  });

  test("reset forgets everything", () => {
    const m = meter(steps(100, 1));
    m.reset();
    expect(m.snapshot()).toMatchObject({ reports: 0, hzAvg: null, hzMax: null, stalls: 0, gaps: 0, recent: [] });
    near(meter(steps(50, 2)).snapshot().hzAvg, 500);
  });
});

describe("ruler", () => {
  test("standard rates sit on evenly spaced marks and the ends clamp", () => {
    expect(rulerPosition(STANDARD_RATES[0])).toBe(0);
    near(rulerPosition(1000), 0.5, 0.0001);
    near(rulerPosition(8000), 1, 0.0001);
    near(rulerPosition(500), 1 / 3, 0.0001);
    expect(rulerPosition(20)).toBe(0);
    expect(rulerPosition(100000)).toBe(1);
    expect(rulerPosition(0)).toBe(0);
    expect(rulerPosition(Number.NaN)).toBe(0);
    expect(rulerPosition(-5)).toBe(0);
  });
});

describe("double click detector", () => {
  test("two presses under 80 ms apart are a failing switch", () => {
    expect(BOUNCE_MS).toBe(80);
    expect(isBounce(1000, 1030)).toBe(true);
    expect(isBounce(1000, 1079.9)).toBe(true);
    expect(isBounce(1000, 1080)).toBe(false);
    expect(isBounce(1000, 1250)).toBe(false);
    expect(isBounce(1000, 1000)).toBe(true);
  });

  test("no previous press, going back in time or garbage is never a bounce", () => {
    expect(isBounce(null, 10)).toBe(false);
    expect(isBounce(undefined, 10)).toBe(false);
    expect(isBounce(100, 50)).toBe(false);
    expect(isBounce(Number.NaN, 50)).toBe(false);
    expect(isBounce(10, Number.NaN)).toBe(false);
    expect(isBounce(10, Number.POSITIVE_INFINITY)).toBe(false);
  });

  test("each button keeps its own count and its own clock", () => {
    let s = emptyButtons();
    s = pressButton(s, 0, 1000);
    s = pressButton(s, 2, 1020); // another button: not a bounce of the left one
    s = pressButton(s, 0, 1040); // left again after 40 ms
    s = pressButton(s, 0, 1500); // a normal second click
    expect(s.map((b) => b.downs)).toEqual([3, 0, 1, 0, 0]);
    expect(s.map((b) => b.bounces)).toEqual([1, 0, 0, 0, 0]);
    expect(s[0]?.gapMs).toBe(40);
    expect(s[2]?.gapMs).toBeNull();
  });

  test("buttons that do not exist change nothing", () => {
    const s = emptyButtons();
    expect(pressButton(s, 5, 10)).toBe(s);
    expect(pressButton(s, -1, 10)).toBe(s);
    expect(pressButton(s, 1.5, 10)).toBe(s);
    expect(pressButton(s, Number.NaN, 10)).toBe(s);
    expect(pressButton(s, 0, Number.NaN)).toBe(s);
    expect(s.every((b) => b.downs === 0)).toBe(true);
  });

  test("the stats are never mutated in place", () => {
    const before = emptyButtons();
    const after = pressButton(before, 3, 5);
    expect(before[3]?.downs).toBe(0);
    expect(after[3]?.downs).toBe(1);
    expect(after[3]?.lastDown).toBe(5);
  });
});
