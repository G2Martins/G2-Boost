// Formatting helpers shown on every page (locale-aware, never NaN on bad input).
import { expect, test } from "bun:test";
import { formatBytes, formatMbps, formatMs, redactPaths } from "../src/lib/utils";

test("redactPaths hides the profile folder name, any drive, any case", () => {
  expect(redactPaths("C:\\Users\\Maria Silva\\AppData\\Local\\x")).toBe("%USERPROFILE%\\AppData\\Local\\x");
  expect(redactPaths("d:\\users\\joao")).toBe("%USERPROFILE%");
  expect(redactPaths("C:\\Program Files\\Steam")).toBe("C:\\Program Files\\Steam");
});

test("formatBytes picks the unit and the locale separator", () => {
  expect(formatBytes(0, "pt-BR")).toBe("0 B");
  expect(formatBytes(1536, "pt-BR")).toBe("1,5 KB");
  expect(formatBytes(1536, "en")).toBe("1.5 KB");
  expect(formatBytes(5 * 1024 ** 3, "en")).toBe("5 GB");
  expect(formatBytes(3 * 1024 ** 5, "en")).toBe("3,072 TB");
  expect(formatBytes(-1, "en")).toBe("—");
  expect(formatBytes(Number.NaN, "en")).toBe("—");
});

test("latency and throughput keep one decimal only below 10", () => {
  expect(formatMs(4.26, "pt-BR")).toBe("4,3 ms");
  expect(formatMs(42.6, "pt-BR")).toBe("43 ms");
  expect(formatMbps(9.94, "en")).toBe("9.9 Mbps");
  expect(formatMbps(940.4, "en")).toBe("940 Mbps");
  expect(formatMs(Number.POSITIVE_INFINITY, "en")).toBe("—");
});
