/**
 * Cloudflare speed test engine (free, no key). Endpoints verified 2026-10-14:
 *   GET  https://speed.cloudflare.com/__down?bytes=N   (CORS *, cf-meta-* headers exposed)
 *   POST https://speed.cloudflare.com/__up
 * Measures idle latency/jitter, download, upload, loaded latency and a Waveform-style
 * bufferbloat grade. Everything runs in the webview; nothing is persisted here.
 */

const BASE = "https://speed.cloudflare.com";

export type Phase = "latency" | "download" | "upload" | "done";

export type Meta = {
  ip?: string;
  asn?: string;
  isp?: string;
  city?: string;
  country?: string;
  colo?: string;
};

export type SpeedResult = {
  meta: Meta;
  latencyMs: number; // median idle
  jitterMs: number;
  downloadMbps: number;
  uploadMbps: number;
  loadedDownMs: number; // median latency during download
  loadedUpMs: number;
  bytesDown: number;
  bytesUp: number;
  grade: Grade;
};

export type Grade = "A+" | "A" | "B" | "C" | "D" | "F";

export type Progress = {
  phase: Phase;
  downloadMbps?: number;
  uploadMbps?: number;
  latencyMs?: number;
  meta?: Meta;
};

const median = (xs: number[]) => {
  if (xs.length === 0) return Number.NaN;
  const s = [...xs].sort((a, b) => a - b);
  const m = Math.floor(s.length / 2);
  return s.length % 2 ? (s[m] as number) : ((s[m - 1] as number) + (s[m] as number)) / 2;
};

const jitterOf = (xs: number[]) => {
  if (xs.length < 2) return 0;
  let acc = 0;
  for (let i = 1; i < xs.length; i++) acc += Math.abs((xs[i] as number) - (xs[i - 1] as number));
  return acc / (xs.length - 1);
};

/** Waveform bufferbloat scale: delta = max(loaded) - idle. */
export function gradeOf(deltaMs: number): Grade {
  if (deltaMs < 5) return "A+";
  if (deltaMs < 30) return "A";
  if (deltaMs < 60) return "B";
  if (deltaMs < 200) return "C";
  if (deltaMs < 400) return "D";
  return "F";
}

async function pingOnce(signal?: AbortSignal): Promise<{ ms: number; res: Response }> {
  const t0 = performance.now();
  const res = await fetch(`${BASE}/__down?bytes=0&r=${Math.random()}`, {
    cache: "no-store",
    signal,
  });
  await res.arrayBuffer();
  return { ms: performance.now() - t0, res };
}

function metaFrom(res: Response): Meta {
  const h = (k: string) => res.headers.get(k) ?? undefined;
  return {
    ip: h("cf-meta-ip"),
    asn: h("cf-meta-asn"),
    city: h("cf-meta-city"),
    country: h("cf-meta-country"),
    colo: h("cf-meta-colo"),
  };
}

async function ispFromMeta(signal?: AbortSignal): Promise<string | undefined> {
  try {
    const res = await fetch(`${BASE}/meta`, { cache: "no-store", signal });
    const j = (await res.json()) as { asOrganization?: string };
    return j.asOrganization;
  } catch {
    return undefined;
  }
}

/** Runs N parallel streams for `durationMs`, returning Mbps + bytes, while sampling latency. */
async function downloadPhase(
  durationMs: number,
  streams: number,
  onTick: (mbps: number, latency: number[]) => void,
  signal: AbortSignal,
): Promise<{ mbps: number; bytes: number; loaded: number[] }> {
  const start = performance.now();
  let bytes = 0;
  let bytesAfterWarmup = 0;
  let warmupEnd = 0;
  const loaded: number[] = [];
  const ctrl = new AbortController();
  signal.addEventListener("abort", () => ctrl.abort());
  const deadline = start + durationMs;

  const stream = async () => {
    while (performance.now() < deadline && !ctrl.signal.aborted) {
      const res = await fetch(`${BASE}/__down?bytes=26214400&r=${Math.random()}`, {
        cache: "no-store",
        signal: ctrl.signal,
      });
      const reader = res.body?.getReader();
      if (!reader) return;
      while (performance.now() < deadline) {
        const { done, value } = await reader.read();
        if (done) break;
        bytes += value.byteLength;
        if (warmupEnd && performance.now() > warmupEnd) bytesAfterWarmup += value.byteLength;
      }
      await reader.cancel().catch(() => undefined);
    }
  };
  // 600 ms warm-up is excluded from the throughput figure.
  warmupEnd = start + 600;
  const sampler = (async () => {
    while (performance.now() < deadline && !ctrl.signal.aborted) {
      try {
        const { ms } = await pingOnce(ctrl.signal);
        loaded.push(ms);
      } catch {
        /* aborted */
      }
      const elapsed = (performance.now() - warmupEnd) / 1000;
      if (elapsed > 0.2) onTick((bytesAfterWarmup * 8) / elapsed / 1e6, loaded);
      await new Promise((r) => setTimeout(r, 400));
    }
  })();
  await Promise.all([...Array.from({ length: streams }, stream), sampler]);
  ctrl.abort();
  const elapsed = (Math.min(performance.now(), deadline) - warmupEnd) / 1000;
  return { mbps: (bytesAfterWarmup * 8) / Math.max(elapsed, 0.1) / 1e6, bytes, loaded };
}

/** Upload with XHR (fetch has no upload progress). */
async function uploadPhase(
  durationMs: number,
  streams: number,
  onTick: (mbps: number, latency: number[]) => void,
  signal: AbortSignal,
): Promise<{ mbps: number; bytes: number; loaded: number[] }> {
  const start = performance.now();
  const warmupEnd = start + 600;
  const deadline = start + durationMs;
  const payload = new Uint8Array(4 * 1024 * 1024); // 4 MB of zeros per request
  let bytes = 0;
  let bytesAfterWarmup = 0;
  const loaded: number[] = [];
  const xhrs = new Set<XMLHttpRequest>();
  let stopped = false;
  const stopAll = () => {
    stopped = true;
    for (const x of xhrs) x.abort();
  };
  signal.addEventListener("abort", stopAll);

  const stream = () =>
    new Promise<void>((resolve) => {
      const next = () => {
        if (stopped || performance.now() >= deadline) return resolve();
        const xhr = new XMLHttpRequest();
        xhrs.add(xhr);
        let last = 0;
        xhr.upload.onprogress = (e) => {
          const delta = e.loaded - last;
          last = e.loaded;
          bytes += delta;
          if (performance.now() > warmupEnd) bytesAfterWarmup += delta;
          if (performance.now() >= deadline) xhr.abort();
        };
        xhr.onloadend = () => {
          xhrs.delete(xhr);
          next();
        };
        xhr.open("POST", `${BASE}/__up?r=${Math.random()}`);
        xhr.send(payload);
      };
      next();
    });

  const sampler = (async () => {
    while (performance.now() < deadline && !stopped) {
      try {
        const { ms } = await pingOnce(signal);
        loaded.push(ms);
      } catch {
        /* aborted */
      }
      const elapsed = (performance.now() - warmupEnd) / 1000;
      if (elapsed > 0.2) onTick((bytesAfterWarmup * 8) / elapsed / 1e6, loaded);
      await new Promise((r) => setTimeout(r, 400));
    }
  })();
  await Promise.all([...Array.from({ length: streams }, stream), sampler]);
  stopAll();
  const elapsed = (Math.min(performance.now(), deadline) - warmupEnd) / 1000;
  return { mbps: (bytesAfterWarmup * 8) / Math.max(elapsed, 0.1) / 1e6, bytes, loaded };
}

export async function runSpeedTest(
  onProgress: (p: Progress) => void,
  signal: AbortSignal,
): Promise<SpeedResult> {
  onProgress({ phase: "latency" });
  const idle: number[] = [];
  let meta: Meta = {};
  for (let i = 0; i < 10; i++) {
    const { ms, res } = await pingOnce(signal);
    if (i === 0) meta = metaFrom(res);
    if (i > 0) idle.push(ms); // first request pays for the TLS handshake
    onProgress({ phase: "latency", latencyMs: median(idle), meta });
  }
  meta.isp = await ispFromMeta(signal);
  const latencyMs = median(idle);
  const jitterMs = jitterOf(idle);

  onProgress({ phase: "download", latencyMs, meta });
  const down = await downloadPhase(
    9000,
    4,
    (mbps) => onProgress({ phase: "download", downloadMbps: mbps, latencyMs, meta }),
    signal,
  );

  onProgress({ phase: "upload", downloadMbps: down.mbps, latencyMs, meta });
  const up = await uploadPhase(
    7000,
    3,
    (mbps) =>
      onProgress({ phase: "upload", downloadMbps: down.mbps, uploadMbps: mbps, latencyMs, meta }),
    signal,
  );

  const loadedDownMs = median(down.loaded);
  const loadedUpMs = median(up.loaded);
  const delta = Math.max(loadedDownMs, loadedUpMs) - latencyMs;
  const result: SpeedResult = {
    meta,
    latencyMs,
    jitterMs,
    downloadMbps: down.mbps,
    uploadMbps: up.mbps,
    loadedDownMs,
    loadedUpMs,
    bytesDown: down.bytes,
    bytesUp: up.bytes,
    grade: gradeOf(Number.isFinite(delta) ? delta : 0),
  };
  onProgress({ phase: "done", downloadMbps: down.mbps, uploadMbps: up.mbps, latencyMs, meta });
  return result;
}

/** Activity verdicts (ExitLag "Atividade" table). */
export function activityVerdicts(r: SpeedResult) {
  const loaded = Math.max(r.loadedDownMs, r.loadedUpMs);
  const check = (down: number, up: number, maxLatency: number, maxJitter = Infinity) => ({
    ideal:
      r.downloadMbps >= down &&
      r.uploadMbps >= up &&
      r.latencyMs <= maxLatency &&
      r.jitterMs <= maxJitter,
    loaded: r.downloadMbps >= down && r.uploadMbps >= up && loaded <= maxLatency,
  });
  return {
    web: check(1, 0.2, 300),
    audio: check(0.5, 0.5, 150),
    video4k: check(25, 0.5, 300),
    conf: check(5, 3, 150),
    gaming: check(3, 1, 60, 25),
  };
}
