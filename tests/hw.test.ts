// Hardware monitor helpers: unit formatting, min/max tracking, position in range, CSV rows.
import { expect, test } from "bun:test";
import type { HwSnapshot } from "../src/lib/backend";
import { csvRows, deviceIds, formatSensor, position, type Stat, trackStats } from "../src/lib/hw";

const NB = " ";

function snap(...values: number[]): HwSnapshot {
  return {
    seq: 1,
    ts: 0,
    count: values.length,
    external: null,
    cpu_temp: false,
    devices: [
      {
        id: "cpu",
        label: "Intel i5",
        key: null,
        n: null,
        kind: "cpu",
        groups: [
          {
            id: "cpu/load",
            label: "Load",
            key: "load",
            sensors: values.map((value, i) => ({
              id: `cpu/load/thread${i}`,
              label: `Thread ${i}`,
              key: "thread",
              n: i,
              value,
              unit: "%",
            })),
          },
        ],
      },
    ],
  };
}

test("each unit keeps its own fixed decimals and the locale separator", () => {
  expect(formatSensor(12.5, "%", "pt-BR")).toBe("12,5%");
  expect(formatSensor(12.5, "%", "en")).toBe("12.5%");
  expect(formatSensor(0, "%", "pt-BR")).toBe("0,0%");
  expect(formatSensor(45.3, "°C", "pt-BR")).toBe(`45,3${NB}°C`);
  expect(formatSensor(12.096, "V", "pt-BR")).toBe(`12,096${NB}V`);
  expect(formatSensor(4100.4, "MHz", "pt-BR")).toBe(`4.100${NB}MHz`);
  expect(formatSensor(1200, "RPM", "en")).toBe(`1,200${NB}RPM`);
  expect(formatSensor(41.234, "W", "pt-BR")).toBe(`41,2${NB}W`);
});

test("bytes and rates scale themselves", () => {
  expect(formatSensor(1536, "B/s", "pt-BR")).toBe("1,5 KB/s");
  expect(formatSensor(5 * 1024 ** 3, "B", "en")).toBe("5 GB");
  expect(formatSensor(0, "B/s", "en")).toBe("0 B/s");
});

test("units the tool invents get sensible digits and bad numbers never print NaN", () => {
  expect(formatSensor(3, "x", "pt-BR")).toBe(`3,00${NB}x`);
  expect(formatSensor(1000, "Mb/s", "pt-BR")).toBe(`1.000${NB}Mb/s`);
  expect(formatSensor(12.34, "GT/s", "en")).toBe(`12.3${NB}GT/s`);
  expect(formatSensor(7.891, "foo", "en")).toBe(`7.89${NB}foo`);
  expect(formatSensor(5, "", "en")).toBe("5.00");
  for (const bad of [Number.NaN, Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY]) {
    expect(formatSensor(bad, "%", "en")).toBe("—");
  }
});

test("min and max grow with the samples and survive a reset-and-reseed", () => {
  const stats = new Map<string, Stat>();
  trackStats(stats, snap(10, 50));
  expect(stats.get("cpu/load/thread0")).toEqual({ min: 10, max: 10 });
  trackStats(stats, snap(30, 20));
  trackStats(stats, snap(5, 70));
  expect(stats.get("cpu/load/thread0")).toEqual({ min: 5, max: 30 });
  expect(stats.get("cpu/load/thread1")).toEqual({ min: 20, max: 70 });
  stats.clear();
  trackStats(stats, snap(33));
  expect(stats.get("cpu/load/thread0")).toEqual({ min: 33, max: 33 });
  expect(stats.has("cpu/load/thread1")).toBe(false);
});

test("position: percentages use 0..100, the rest the range seen so far", () => {
  expect(position(25, "%", undefined)).toBe(0.25);
  expect(position(150, "%", undefined)).toBe(1);
  expect(position(-5, "%", undefined)).toBe(0);
  expect(position(55, "°C", { min: 40, max: 60 })).toBe(0.75);
  expect(position(40, "°C", { min: 40, max: 60 })).toBe(0);
  expect(position(41, "°C", { min: 41, max: 41 })).toBeNull();
  expect(position(41, "°C", undefined)).toBeNull();
  expect(position(99, "RPM", { min: 100, max: 200 })).toBe(0);
});

test("csv rows carry the labels the page shows, the stats and null for what is missing", () => {
  const stats = new Map<string, Stat>();
  trackStats(stats, snap(10, 20));
  trackStats(stats, snap(30, 20));
  const label = (scope: string, x: { key: string | null; n?: number | null; label: string }) =>
    x.key ? `${scope}:${x.key}:${x.n ?? ""}` : x.label;
  const rows = csvRows(snap(30, 20), stats, label);
  expect(rows).toHaveLength(2);
  expect(rows[0]).toEqual({
    device: "Intel i5",
    group: "g:load:",
    sensor: "s:thread:0",
    value: 30,
    min: 10,
    max: 30,
    unit: "%",
  });
  const bare = csvRows(snap(1), new Map(), label);
  expect(bare[0]?.min).toBeNull();
  expect(bare[0]?.max).toBeNull();
  expect(deviceIds(snap(1))).toEqual(["cpu"]);
  expect(deviceIds(null)).toEqual([]);
});
