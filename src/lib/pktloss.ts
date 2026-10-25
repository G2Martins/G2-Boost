import type { PacketLossProfile, PacketLossSummary } from "@/lib/backend";
import type { I18nKey } from "@/lib/i18n";
import type { RouteAnalysis, RouteNode } from "@/lib/route";

/**
 * Burst sizes, as the core fixes them (`pktloss::Profile`; both sides pin the numbers in tests).
 * The UI needs the packet count before the first event arrives to lay out the strip.
 */
export const PROFILES: Record<PacketLossProfile, { rate: number; seconds: number }> = {
  quick: { rate: 10, seconds: 10 },
  standard: { rate: 15, seconds: 20 },
  long: { rate: 10, seconds: 60 },
};

export const packetCount = (p: PacketLossProfile) => PROFILES[p].rate * PROFILES[p].seconds;

/** A reply slower than this is late whatever the usual time is (the core's `LATE_MS`). */
export const LATE_MS = 150;

/* ---------------- one packet ---------------- */

/** What the strip knows about a packet that already has an outcome. */
export type Cell = { rtt: number | null; late: boolean; ooo: boolean };
export type CellState = "pending" | "ok" | "late" | "lost";

export function cellState(c: Cell | undefined): CellState {
  if (!c) return "pending";
  if (c.rtt === null) return "lost";
  return c.late ? "late" : "ok";
}

/* ---------------- the verdict, in plain words ---------------- */

export type TargetKind = "gateway" | "internet" | "custom";

export type Verdict = {
  tone: "success" | "warning" | "danger" | "neutral";
  title: I18nKey;
  body: I18nKey;
  /** Where to look next; null when there is nothing to add. */
  advice: I18nKey | null;
};

/** A finished run needs at least this many packets to say anything. */
export const MIN_FOR_VERDICT = 20;
/** Loss (%) from which games start to notice. */
const NOTICEABLE = 1;
/** Loss (%) that is plainly bad. */
const HEAVY = 3;
/** Back to back losses that feel like a freeze even when the total is small. */
const FREEZE_RUN = 5;
/** Share of late replies (%) and jitter (ms) that make a clean line "unstable". */
const LATE_SHARE = 5;
const JITTER = 30;
/** One stray reordered packet is the path being a path; a steady trickle is not. */
const OOO_SHARE = 1;
const OOO_MIN = 3;

export function verdict(s: PacketLossSummary, kind: TargetKind, finished: boolean): Verdict {
  const lossy: I18nKey =
    kind === "gateway" ? "tools.loss.advice.gateway" : "tools.loss.advice.beyond";
  // The verdict comes when the test ends: a run that is still going can swing either way.
  if (!finished) {
    return {
      tone: "neutral",
      title: "tools.loss.verdict.collecting",
      body: "tools.loss.verdict.collecting.body",
      advice: null,
    };
  }
  if (s.sent < MIN_FOR_VERDICT) {
    return {
      tone: "neutral",
      title: "tools.loss.verdict.short",
      body: "tools.loss.verdict.short.body",
      advice: null,
    };
  }
  if (s.received === 0) {
    return {
      tone: "danger",
      title: "tools.loss.verdict.noanswer",
      body: "tools.loss.verdict.noanswer.body",
      advice: "tools.loss.advice.noanswer",
    };
  }
  if (s.loss_pct >= HEAVY || s.longest_loss_run >= FREEZE_RUN) {
    return {
      tone: "danger",
      title: "tools.loss.verdict.heavy",
      body: "tools.loss.verdict.heavy.body",
      advice: lossy,
    };
  }
  if (s.loss_pct > NOTICEABLE) {
    return {
      tone: "warning",
      title: "tools.loss.verdict.noticeable",
      body: "tools.loss.verdict.noticeable.body",
      advice: lossy,
    };
  }
  if (s.loss_pct > 0) {
    return {
      tone: "success",
      title: "tools.loss.verdict.minor",
      body: "tools.loss.verdict.minor.body",
      advice: null,
    };
  }
  const lateShare = (100 * s.late) / s.sent;
  const oooShare = (100 * s.out_of_order) / s.sent;
  const shuffled = s.out_of_order >= OOO_MIN && oooShare > OOO_SHARE;
  if (lateShare > LATE_SHARE || (s.jitter_ms ?? 0) > JITTER || shuffled) {
    return {
      tone: "warning",
      title: "tools.loss.verdict.unstable",
      body: "tools.loss.verdict.unstable.body",
      advice: "tools.loss.advice.unstable",
    };
  }
  return {
    tone: "success",
    title: "tools.loss.verdict.clean",
    body: "tools.loss.verdict.clean.body",
    advice: null,
  };
}

/* ---------------- the route: real loss or a router that ignores ping ---------------- */

export type RouteVerdict =
  /** Loss that reaches the destination, starting at `hop`. */
  | { kind: "real"; hop: RouteNode }
  /** The burst lost packets but the trace (few probes per hop) did not show where. */
  | { kind: "unseen" }
  /** Some hops in the middle lost answers while the destination answered: they ignore ping. */
  | { kind: "ignores_ping"; hops: number }
  | { kind: "clean" };

/**
 * Same honest rule as `analyzeRoute`: only loss that persists to the destination is real. A
 * router in the middle that drops its own replies but forwards traffic is not a problem.
 */
export function routeVerdict(a: RouteAnalysis, burstLossPct: number | null): RouteVerdict {
  const from = a.lossFrom === null ? undefined : a.nodes[a.lossFrom];
  if (from) return { kind: "real", hop: from };
  if (burstLossPct !== null && burstLossPct > 0) return { kind: "unseen" };
  const hops = a.nodes.filter((n) => n.loss > 0).length;
  return hops > 0 ? { kind: "ignores_ping", hops } : { kind: "clean" };
}
