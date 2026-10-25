import { describe, expect, test } from "bun:test";
import {
  estimateRefresh,
  LATE_FACTOR,
  missedFrames,
  nearestStandardHz,
  PAUSE_FACTOR,
  pixelsPerFrame,
} from "../src/lib/refresh";

const frames = (n: number, ms: number) => Array.from({ length: n }, () => ms);
const near = (got: number | null, want: number, tol = 0.01) => {
  expect(got).not.toBeNull();
  expect(Math.abs((got as number) - want)).toBeLessThanOrEqual(tol);
};

describe("refresh rate", () => {
  test("steady frames give the rate", () => {
    near(estimateRefresh(frames(120, 1000 / 144)).hz, 144);
    near(estimateRefresh(frames(120, 1000 / 60)).hz, 60);
    near(estimateRefresh(frames(120, 1000 / 240)).hz, 240);
    const e = estimateRefresh(frames(120, 1000 / 144));
    expect(e.dropped).toBe(0);
    expect(e.frames).toBe(120);
    near(e.frameMs, 1000 / 144, 0.0001);
  });

  test("a little noise around the frame time does not change the rate", () => {
    const noisy = Array.from({ length: 240 }, (_, i) => 6.944 + (i % 3 === 0 ? 0.08 : i % 3 === 1 ? -0.06 : 0));
    near(estimateRefresh(noisy).hz, 144, 0.5);
    expect(estimateRefresh(noisy).dropped).toBe(0);
  });

  test("a late frame counts the slots it missed", () => {
    const ms = 1000 / 144;
    const e = estimateRefresh([...frames(60, ms), ms * 3, ...frames(60, ms)]);
    expect(e.dropped).toBe(2);
    // The rate still reads the display, not the drop.
    near(e.hz, 144, 0.01);
    near(e.worstMs, ms * 3, 0.0001);
    // 1.6x is late and missed one; 1.4x is just jitter.
    expect(estimateRefresh([...frames(60, 10), 16]).dropped).toBe(1);
    expect(estimateRefresh([...frames(60, 10), 14]).dropped).toBe(0);
  });

  test("a pause is not a drop and does not move the rate", () => {
    const ms = 1000 / 60;
    const e = estimateRefresh([...frames(60, ms), 3000, ...frames(60, ms)]);
    expect(e.dropped).toBe(0);
    expect(e.frames).toBe(120);
    near(e.hz, 60, 0.01);
    expect(e.worstMs).toBeLessThan(ms * 1.01);
  });

  test("garbage frame times are ignored", () => {
    const e = estimateRefresh([Number.NaN, -5, 0, Number.POSITIVE_INFINITY, ...frames(30, 10)]);
    near(e.hz, 100);
    expect(e.frames).toBe(30);
    expect(estimateRefresh([])).toEqual({ hz: null, frameMs: null, dropped: 0, frames: 0, worstMs: null });
    expect(estimateRefresh([Number.NaN, 0]).hz).toBeNull();
  });

  test("a single frame is enough to say something, a lot of drops do not break it", () => {
    near(estimateRefresh([16.667]).hz, 60, 0.01);
    const bad = estimateRefresh([...frames(10, 10), ...frames(10, 40)]);
    expect(bad.hz).not.toBeNull();
    expect(Number.isFinite(bad.hz as number)).toBe(true);
  });
});

describe("missed frames", () => {
  test("on time is zero, the thresholds are the documented ones", () => {
    expect(LATE_FACTOR).toBe(1.5);
    expect(PAUSE_FACTOR).toBe(10);
    expect(missedFrames(10, 10)).toBe(0);
    expect(missedFrames(15, 10)).toBe(0);
    expect(missedFrames(15.1, 10)).toBe(1);
    expect(missedFrames(20, 10)).toBe(1);
    expect(missedFrames(30, 10)).toBe(2);
    expect(missedFrames(100, 10)).toBe(9);
  });

  test("pauses and nonsense are not drops", () => {
    expect(missedFrames(100.1, 10)).toBe(0);
    expect(missedFrames(5000, 10)).toBe(0);
    expect(missedFrames(Number.NaN, 10)).toBe(0);
    expect(missedFrames(10, 0)).toBe(0);
    expect(missedFrames(10, Number.NaN)).toBe(0);
    expect(missedFrames(-3, 10)).toBe(0);
  });
});

describe("standard rates", () => {
  test("a measurement within 1.5% reads as the rate it is", () => {
    expect(nearestStandardHz(59.94)).toBe(60);
    expect(nearestStandardHz(143.7)).toBe(144);
    expect(nearestStandardHz(164.9)).toBe(165);
    expect(nearestStandardHz(239.76)).toBe(240);
    expect(nearestStandardHz(100.5)).toBe(100);
    expect(nearestStandardHz(60)).toBe(60);
  });

  test("anything else is not guessed", () => {
    expect(nearestStandardHz(87)).toBeNull();
    expect(nearestStandardHz(52.5)).toBeNull();
    expect(nearestStandardHz(null)).toBeNull();
    expect(nearestStandardHz(Number.NaN)).toBeNull();
    expect(nearestStandardHz(0)).toBeNull();
    expect(nearestStandardHz(-60)).toBeNull();
    expect(nearestStandardHz(Number.POSITIVE_INFINITY)).toBeNull();
  });
});

describe("pixels per frame", () => {
  test("speed over rate", () => {
    near(pixelsPerFrame(960, 60), 16);
    near(pixelsPerFrame(960, 144), 6.6667, 0.001);
    near(pixelsPerFrame(1920, 240), 8);
  });

  test("no rate yet, no answer", () => {
    expect(pixelsPerFrame(960, null)).toBeNull();
    expect(pixelsPerFrame(960, 0)).toBeNull();
    expect(pixelsPerFrame(Number.NaN, 60)).toBeNull();
  });
});
