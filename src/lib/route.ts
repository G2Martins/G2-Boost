import type { Hop } from "@/lib/backend";

/** Where a hop sits: your own network, your provider's edge, or the internet beyond. */
export type Zone = "home" | "isp" | "internet";

export type RouteNode = {
  ttl: number;
  addr: string | null;
  /** Average of the answered probes (ms), null when the hop did not answer. */
  avg: number | null;
  /** Share of probes without an answer (0–100). */
  loss: number;
  zone: Zone;
  /** Latency this hop adds over the previous answering hop (ms, never negative). */
  delta: number | null;
};

export type RouteAnalysis = {
  nodes: RouteNode[];
  /** Index of the hop where latency jumps and stays up (a real slow link), or null. */
  worst: number | null;
  /** Latency to the last answering hop. */
  total: number | null;
  /** Index from which packet loss persists to the destination (real loss), or null. */
  lossFrom: number | null;
};

function v4(addr: string): number[] | null {
  const p = addr.split(".").map(Number);
  return p.length === 4 && p.every((n) => Number.isInteger(n) && n >= 0 && n <= 255) ? p : null;
}

function isPrivate(addr: string): boolean {
  const p = v4(addr);
  if (!p) return false;
  const [a, b] = p as [number, number, number, number];
  return a === 10 || (a === 172 && b >= 16 && b <= 31) || (a === 192 && b === 168) || a === 127;
}

function isCgnat(addr: string): boolean {
  const p = v4(addr);
  return !!p && p[0] === 100 && (p[1] ?? 0) >= 64 && (p[1] ?? 0) <= 127;
}

const mean = (xs: number[]) => (xs.length ? xs.reduce((a, b) => a + b, 0) / xs.length : null);

/**
 * Turn raw traceroute hops into what a gamer can act on. Routers often answer ICMP slowly
 * (or not at all) while forwarding traffic fine, so a latency jump only counts when the hops
 * after it stay up, and loss only counts when it reaches the destination.
 */
export function analyzeRoute(hops: Hop[]): RouteAnalysis {
  let firstPublic = -1;
  let prev: number | null = null;
  const nodes: RouteNode[] = hops.map((h, i) => {
    const answered = h.rtts_ms.filter((r): r is number => r !== null);
    const avg = mean(answered);
    const loss = h.rtts_ms.length
      ? Math.round((100 * (h.rtts_ms.length - answered.length)) / h.rtts_ms.length)
      : 100;
    let zone: Zone = "internet";
    if (h.addr && isPrivate(h.addr)) zone = "home";
    else if (h.addr && isCgnat(h.addr)) zone = "isp";
    else if (h.addr) {
      if (firstPublic < 0) firstPublic = i;
      // The first public hops are almost always the provider's (no ASN lookup offline).
      if (i - firstPublic < 2) zone = "isp";
    }
    const delta = avg !== null && prev !== null ? Math.max(0, avg - prev) : null;
    if (avg !== null) prev = avg;
    return { ttl: h.ttl, addr: h.addr, avg, loss, zone, delta };
  });

  // A jump is real when every later answering hop stays at least 70 % of the way up.
  let worst: number | null = null;
  let worstDelta = 5;
  nodes.forEach((n, i) => {
    const delta = n.delta;
    if (delta === null || n.avg === null || delta <= worstDelta) return;
    const base = n.avg - delta;
    const later = nodes.slice(i + 1).filter((x) => x.avg !== null);
    const persists = later.every((x) => (x.avg ?? 0) >= base + delta * 0.7);
    if (persists) {
      worst = i;
      worstDelta = delta;
    }
  });

  const answering = nodes.filter((n) => n.avg !== null);
  const last = answering[answering.length - 1];
  let lossFrom: number | null = null;
  if (last && last.loss > 0) {
    // Walk back from the end while hops keep losing packets.
    for (let i = nodes.length - 1; i >= 0; i--) {
      const n = nodes[i];
      if (!n || n.avg === null) continue;
      if (n.loss > 0) lossFrom = i;
      else break;
    }
  }
  return { nodes, worst, total: last?.avg ?? null, lossFrom };
}
