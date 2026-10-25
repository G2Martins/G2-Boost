import { describe, expect, test } from "bun:test";
import type { Hop, PacketLossSummary } from "../src/lib/backend";
import {
  cellState,
  MIN_FOR_VERDICT,
  packetCount,
  PROFILES,
  routeVerdict,
  verdict,
} from "../src/lib/pktloss";
import { analyzeRoute } from "../src/lib/route";

/** A finished run of `sent` packets with the given trouble; the rest is calm. */
const run = (over: Partial<PacketLossSummary> = {}): PacketLossSummary => {
  const sent = over.sent ?? 300;
  const lost = over.lost ?? 0;
  return {
    sent,
    received: sent - lost,
    lost,
    loss_pct: (100 * lost) / sent,
    late: 0,
    out_of_order: 0,
    avg_ms: 20,
    min_ms: 18,
    max_ms: 30,
    median_ms: 20,
    p95_ms: 25,
    jitter_ms: 2,
    longest_loss_run: lost > 0 ? 1 : 0,
    ...over,
  };
};

describe("profiles", () => {
  test("match the core's three bursts", () => {
    expect(PROFILES).toEqual({
      quick: { rate: 10, seconds: 10 },
      standard: { rate: 15, seconds: 20 },
      long: { rate: 10, seconds: 60 },
    });
    expect(packetCount("quick")).toBe(100);
    expect(packetCount("standard")).toBe(300);
    expect(packetCount("long")).toBe(600);
  });
});

describe("strip cells", () => {
  test("pending until there is an outcome, then ok, late or lost", () => {
    expect(cellState(undefined)).toBe("pending");
    expect(cellState({ rtt: 20, late: false, ooo: false })).toBe("ok");
    expect(cellState({ rtt: 400, late: true, ooo: false })).toBe("late");
    expect(cellState({ rtt: null, late: false, ooo: false })).toBe("lost");
    // Out of order is a flag on top of ok or late, not a state of its own.
    expect(cellState({ rtt: 20, late: false, ooo: true })).toBe("ok");
    // A lost packet cannot be late, whatever the flag says.
    expect(cellState({ rtt: null, late: true, ooo: false })).toBe("lost");
  });
});

describe("verdict", () => {
  test("a clean line is good news", () => {
    const v = verdict(run(), "internet", true);
    expect(v.tone).toBe("success");
    expect(v.title).toBe("tools.loss.verdict.clean");
    expect(v.advice).toBeNull();
  });

  test("a run that is still going gets no verdict, however it looks", () => {
    const bad = verdict(run({ sent: 200, lost: 40 }), "internet", false);
    expect(bad.title).toBe("tools.loss.verdict.collecting");
    expect(bad.tone).toBe("neutral");
    expect(bad.advice).toBeNull();
    expect(verdict(run({ sent: 0, received: 0, lost: 0, loss_pct: 0 }), "internet", false).title).toBe(
      "tools.loss.verdict.collecting",
    );
    expect(verdict(run(), "internet", false).title).toBe("tools.loss.verdict.collecting");
  });

  test("a run stopped too early says it is too short instead of guessing", () => {
    expect(MIN_FOR_VERDICT).toBe(20);
    expect(verdict(run({ sent: 10, lost: 3 }), "internet", true).title).toBe("tools.loss.verdict.short");
    expect(verdict(run({ sent: 0, received: 0, lost: 0, loss_pct: 0 }), "internet", true).title).toBe(
      "tools.loss.verdict.short",
    );
    expect(verdict(run({ sent: MIN_FOR_VERDICT - 1, lost: 19 }), "internet", true).tone).toBe("neutral");
    // At the minimum it speaks.
    expect(verdict(run({ sent: MIN_FOR_VERDICT, lost: 0 }), "internet", true).title).toBe("tools.loss.verdict.clean");
    expect(verdict(run({ sent: MIN_FOR_VERDICT, lost: 3 }), "internet", true).tone).toBe("danger");
  });

  test("no reply at all", () => {
    const v = verdict(run({ sent: 100, lost: 100 }), "internet", true);
    expect(v.title).toBe("tools.loss.verdict.noanswer");
    expect(v.tone).toBe("danger");
    expect(verdict(run({ sent: 25, lost: 25 }), "gateway", true).title).toBe("tools.loss.verdict.noanswer");
  });

  test("loss steps: minor, noticeable, heavy", () => {
    expect(verdict(run({ lost: 1 }), "internet", true).title).toBe("tools.loss.verdict.minor"); // 0.3%
    expect(verdict(run({ sent: 100, lost: 1 }), "internet", true).title).toBe("tools.loss.verdict.minor"); // exactly 1%
    expect(verdict(run({ lost: 5 }), "internet", true).title).toBe("tools.loss.verdict.noticeable"); // 1.7%
    expect(verdict(run({ lost: 9 }), "internet", true).title).toBe("tools.loss.verdict.heavy"); // exactly 3%
    expect(verdict(run({ lost: 40 }), "internet", true).tone).toBe("danger");
  });

  test("a freeze counts even when the total is small", () => {
    const v = verdict(run({ sent: 600, lost: 5, longest_loss_run: 5 }), "internet", true); // 0.8%
    expect(v.title).toBe("tools.loss.verdict.heavy");
    expect(verdict(run({ sent: 600, lost: 5, longest_loss_run: 4 }), "internet", true).title).toBe(
      "tools.loss.verdict.minor",
    );
  });

  test("where to look depends on what was probed", () => {
    expect(verdict(run({ lost: 12 }), "gateway", true).advice).toBe("tools.loss.advice.gateway");
    expect(verdict(run({ lost: 12 }), "internet", true).advice).toBe("tools.loss.advice.beyond");
    expect(verdict(run({ lost: 12 }), "custom", true).advice).toBe("tools.loss.advice.beyond");
    expect(verdict(run({ lost: 1 }), "gateway", true).advice).toBeNull();
  });

  test("no loss but a shaky line is called what it is", () => {
    expect(verdict(run({ late: 30 }), "internet", true).title).toBe("tools.loss.verdict.unstable"); // 10% late
    expect(verdict(run({ late: 15 }), "internet", true).title).toBe("tools.loss.verdict.clean"); // exactly 5%
    expect(verdict(run({ jitter_ms: 45 }), "internet", true).title).toBe("tools.loss.verdict.unstable");
    // A stray reordered packet or two is normal; a trickle of them is not (3+ and over 1%).
    expect(verdict(run({ out_of_order: 1 }), "internet", true).title).toBe("tools.loss.verdict.clean");
    expect(verdict(run({ out_of_order: 2 }), "internet", true).title).toBe("tools.loss.verdict.clean");
    expect(verdict(run({ out_of_order: 3 }), "internet", true).title).toBe("tools.loss.verdict.clean"); // exactly 1%
    expect(verdict(run({ out_of_order: 4 }), "internet", true).title).toBe("tools.loss.verdict.unstable");
    expect(verdict(run({ sent: 100, out_of_order: 2 }), "internet", true).title).toBe("tools.loss.verdict.clean");
    expect(verdict(run({ sent: 100, out_of_order: 3 }), "internet", true).title).toBe("tools.loss.verdict.unstable");
    expect(verdict(run({ jitter_ms: null }), "internet", true).title).toBe("tools.loss.verdict.clean");
    expect(verdict(run({ late: 30 }), "internet", true).advice).toBe("tools.loss.advice.unstable");
  });
});

describe("route verdict", () => {
  const hop = (ttl: number, addr: string | null, ...rtts: (number | null)[]): Hop => ({
    ttl,
    addr,
    rtts_ms: rtts,
  });

  test("loss that reaches the destination is real, and points at the first lossy hop", () => {
    const a = analyzeRoute([
      hop(1, "192.168.0.1", 1, 1, 1),
      hop(2, "200.1.1.1", 5, 5, 5),
      hop(3, "200.2.2.2", 7, null, 7),
      hop(4, "8.8.8.8", 9, null, null),
    ]);
    const r = routeVerdict(a, 4);
    expect(r.kind).toBe("real");
    if (r.kind === "real") {
      expect(r.hop.ttl).toBe(3);
      expect(r.hop.addr).toBe("200.2.2.2");
    }
    // Real even when the burst itself saw nothing: the trace is evidence on its own.
    expect(routeVerdict(a, 0).kind).toBe("real");
    expect(routeVerdict(a, null).kind).toBe("real");
  });

  test("a router that only ignores ping is not blamed", () => {
    const a = analyzeRoute([
      hop(1, "192.168.0.1", 1, 1, 1),
      hop(2, "200.1.1.1", null, null, 5),
      hop(3, "8.8.8.8", 9, 9, 9),
    ]);
    expect(routeVerdict(a, 0)).toEqual({ kind: "ignores_ping", hops: 1 });
    expect(routeVerdict(a, null)).toEqual({ kind: "ignores_ping", hops: 1 });
  });

  test("the burst lost packets but the trace saw no pattern: say so, do not guess", () => {
    const a = analyzeRoute([
      hop(1, "192.168.0.1", 1, 1, 1),
      hop(2, "200.1.1.1", 5, 5, 5),
      hop(3, "8.8.8.8", 9, 9, 9),
    ]);
    expect(routeVerdict(a, 2.5)).toEqual({ kind: "unseen" });
    expect(routeVerdict(a, 0)).toEqual({ kind: "clean" });
    expect(routeVerdict(a, null)).toEqual({ kind: "clean" });
  });

  test("a trace with nothing in it is clean, never a crash", () => {
    expect(routeVerdict(analyzeRoute([]), null)).toEqual({ kind: "clean" });
    expect(routeVerdict(analyzeRoute([hop(1, null, null, null)]), 0).kind).toBe("ignores_ping");
  });
});
