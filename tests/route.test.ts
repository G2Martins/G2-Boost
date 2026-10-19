import { expect, test } from "bun:test";
import type { Hop } from "../src/lib/backend";
import { analyzeRoute } from "../src/lib/route";

const hop = (ttl: number, addr: string | null, ...rtts: (number | null)[]): Hop => ({ ttl, addr, rtts_ms: rtts });

test("zones: home, provider edge, internet", () => {
  const r = analyzeRoute([
    hop(1, "192.168.0.1", 1, 1, 1),
    hop(2, "100.70.0.1", 4, 4, 4),
    hop(3, "200.1.1.1", 5, 5, 5),
    hop(4, "200.1.2.1", 6, 6, 6),
    hop(5, "8.8.8.8", 12, 12, 12),
  ]);
  expect(r.nodes.map((n) => n.zone)).toEqual(["home", "isp", "isp", "isp", "internet"]);
  expect(r.total).toBe(12);
  expect(r.nodes[4]?.delta).toBe(6);
});

test("a real slow link is the hop where latency jumps and stays up", () => {
  const r = analyzeRoute([
    hop(1, "192.168.0.1", 1, 1, 1),
    hop(2, "200.1.1.1", 5, 5, 5),
    hop(3, "200.9.9.9", 40, 41, 39),
    hop(4, "8.8.8.8", 42, 42, 42),
  ]);
  expect(r.worst).toBe(2);
});

test("a router that only answers ICMP slowly is not blamed", () => {
  const r = analyzeRoute([
    hop(1, "192.168.0.1", 1, 1, 1),
    hop(2, "200.1.1.1", 90, 95, 92),
    hop(3, "200.2.2.2", 8, 8, 8),
    hop(4, "8.8.8.8", 9, 9, 9),
  ]);
  expect(r.worst).toBeNull();
});

test("loss counts only when it reaches the destination", () => {
  const middleOnly = analyzeRoute([
    hop(1, "192.168.0.1", 1, 1, 1),
    hop(2, "200.1.1.1", null, null, 5),
    hop(3, "8.8.8.8", 9, 9, 9),
  ]);
  expect(middleOnly.lossFrom).toBeNull();
  const real = analyzeRoute([
    hop(1, "192.168.0.1", 1, 1, 1),
    hop(2, "200.1.1.1", 5, 5, 5),
    hop(3, "200.2.2.2", 7, null, 7),
    hop(4, "8.8.8.8", 9, null, null),
  ]);
  expect(real.lossFrom).toBe(2);
  expect(analyzeRoute([]).total).toBeNull();
  expect(analyzeRoute([hop(1, null, null, null)]).nodes[0]?.loss).toBe(100);
});
