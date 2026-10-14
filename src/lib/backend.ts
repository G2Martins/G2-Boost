import { invoke } from "@tauri-apps/api/core";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";

/** True inside the Tauri webview; false in a plain browser (`bun run dev`). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export type AppInfo = {
  version: string;
  core_version: string;
  os: string;
  elevated: boolean;
  start_route: string | null;
  start_tab: string | null;
  autorun: boolean;
};

export type DriveInfo = {
  letter: string;
  kind: "fixed" | "removable" | "network" | "cdrom" | "other";
  total_bytes: number;
  free_bytes: number;
};

export type MemoryStatus = {
  total_bytes: number;
  avail_bytes: number;
  load_percent: number;
};

export type Probe =
  | { kind: "reply"; from: string; rtt_ms: number }
  | { kind: "ttl_expired"; from: string; rtt_ms: number }
  | { kind: "timeout" }
  | { kind: "error"; status: number };

export type LatencyStats = {
  samples: number;
  avg_ms: number | null;
  min_ms: number | null;
  max_ms: number | null;
  jitter_ms: number | null;
  loss_pct: number;
  spikes: number;
};

export type PingReport = { host: string; ip: string; probes: Probe[]; stats: LatencyStats };
export type Hop = { ttl: number; addr: string | null; rtts_ms: (number | null)[] };

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T | null> {
  if (!inTauri) return null;
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    // biome-ignore lint/suspicious/noConsole: dev-tools breadcrumb only; the UI shows the unavailable state
    console.warn(`[backend] ${cmd} failed`, e);
    return null;
  }
}

/** Like `call` but surfaces the error message (for user-initiated actions). */
async function callOrThrow<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (!inTauri) throw new Error("app-only");
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw new Error(typeof e === "string" ? e : e instanceof Error ? e.message : String(e));
  }
}

export const backend = {
  appInfo: () => call<AppInfo>("app_info"),
  listDrives: () => call<DriveInfo[]>("list_drives"),
  memoryStatus: () => call<MemoryStatus>("memory_status"),
  pingHost: (host: string, count: number, timeoutMs: number, intervalMs: number) =>
    callOrThrow<PingReport>("ping_host", { host, count, timeoutMs, intervalMs }),
  traceHost: (host: string, maxHops: number, probes: number, timeoutMs: number) =>
    callOrThrow<Hop[]>("trace_host", { host, maxHops, probes, timeoutMs }),
  /** JSON over the Rust HTTP client (scoped in capabilities); plain fetch in the browser. */
  fetchJson: async <T>(url: string): Promise<T> => {
    const res = inTauri ? await tauriFetch(url) : await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return (await res.json()) as T;
  },
};
