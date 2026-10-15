import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";

/** True inside the Tauri webview; false in a plain browser (`bun run dev`). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export type AppInfo = {
  version: string;
  core_version: string;
  os: string;
  elevated: boolean;
  user: string;
  start_route: string | null;
  start_tab: string | null;
  autorun: boolean;
  scan_path: string | null;
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

export type NodeView = {
  name: string;
  size: number;
  is_dir: boolean;
  files: number;
  mtime: number;
  ids: number[];
  children: NodeView[];
  others_size: number;
  others_count: number;
};
export type BigFile = { size: number; path: string; mtime: number };
export type ScanSummary = {
  root: string;
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  elapsed_ms: number;
  cancelled: boolean;
};
export type ScanProgressEvent = {
  files: number;
  dirs: number;
  bytes: number;
  errors: number;
  current: string;
};

export type TweakStatus = "on" | "off" | "partial" | "unavailable";
export type FxLevel = "appearance" | "minimal" | "balanced" | "performance";
export type ServiceState = {
  name: string;
  start: "auto" | "manual" | "disabled" | "other";
  running: boolean;
};
export type TweakState = {
  id: string;
  admin: boolean;
  beta: boolean;
  category: TweakCategory;
  status: TweakStatus;
  has_backup: boolean;
  level: FxLevel | null;
  detail: string | null;
  services: ServiceState[];
};
export type PurgeResult = { before_avail: number; after_avail: number; freed_bytes: number };

export type PreviewItem = {
  id: string;
  bytes: number;
  files: number;
  measurable: boolean;
  missing: boolean;
  needs_admin: boolean;
  blocked_by: string[];
};
export type ConfirmToken = { token: number; ids: string[] };
export type ExecItem = {
  id: string;
  bytes_freed: number;
  files_deleted: number;
  skipped: number;
  error: string | null;
};
export type RunRecord = { at: number; trigger: string; duration_ms: number; items: ExecItem[] };
export type CleanupProgress = { done: number; total: number };

export type Schedule =
  | { mode: "off" }
  | { mode: "at_logon" }
  | { mode: "every_hours"; hours: number }
  | { mode: "daily"; hour: number }
  | { mode: "weekly"; weekday: number; hour: number };
export type Settings = {
  close_to_tray: boolean;
  schedule: Schedule;
  routine_categories: string[];
  game_ram_clean: boolean;
  game_exes: string[];
  restore_before_preset: boolean;
};

export type Gpu = {
  name: string;
  vram_bytes: number;
  vendor: "nvidia" | "amd" | "intel" | "other";
};
export type Disk = {
  index: number;
  model: string;
  bus: "nvme" | "sata" | "usb" | "sas" | "raid" | "other";
  size_bytes: number;
  ssd: boolean | null;
  system: boolean;
};
export type Profile = {
  computer: string;
  os: string;
  cpu: string;
  cores_physical: number;
  cores_logical: number;
  ram_bytes: number;
  uptime_secs: number;
  gpus: Gpu[];
  board: string | null;
  system_model: string | null;
  disks: Disk[];
  system_disk_ssd: boolean | null;
  secure_boot: boolean | null;
  tpm: string | null;
  laptop: boolean;
  elevated: boolean;
};
export type TweakCategory = "performance" | "privacy" | "appearance" | "network";
export type Recommendation = { id: string; recommended: boolean; reason: string | null };
export type Facts = {
  system_ssd: boolean | null;
  laptop: boolean;
  gpu_vendor: string | null;
  hags_supported: boolean;
  game_pass: boolean;
  outlook: boolean;
};
export type Score = { recommended: number; active: number; percent: number; pending: string[] };
export type Recommendations = { facts: Facts; items: Recommendation[]; score: Score };
export type RestoreStatus = {
  enabled: boolean | null;
  count: number;
  last: string | null;
  frequency_minutes: number;
};
export type CreateOutcome = "created" | "skipped";
export type MonitorSample = {
  ts: number;
  cpu_pct: number;
  cpu_mhz: number;
  gpu_pct: number;
  gpu_mem_bytes: number;
  gpu_temp_c: number | null;
  ram_used: number;
  ram_total: number;
  disk_pct: number;
  disk_bytes_s: number;
  net_rx_bytes_s: number;
  net_tx_bytes_s: number;
};
export type Game = { name: string; source: "steam" | "exe" | "steamapps" };
export type AutostartStatus = { enabled: boolean };
export type RoutineStatus = {
  last_run: number | null;
  game: Game | null;
  elevated: boolean;
  running: boolean;
};
export type GameEvent = { game: Game; freed_bytes: number; error: string | null };

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

  diskScanStart: (root: string) => callOrThrow<void>("disk_scan_start", { root }),
  diskScanCancel: () => call<void>("disk_scan_cancel"),
  diskSummary: () => call<ScanSummary | null>("disk_summary"),
  diskTree: (ids: number[], depth = 2, maxChildren = 80) =>
    call<NodeView | null>("disk_tree", { ids, depth, maxChildren }),
  diskTopFiles: (n = 50) => call<BigFile[]>("disk_top_files", { n }),

  boostState: () => call<TweakState[]>("boost_state"),
  boostApply: (id: string, on: boolean) => callOrThrow<TweakState>("boost_apply", { id, on }),
  boostVisualFx: (level: FxLevel) => callOrThrow<TweakState>("boost_visual_fx", { level }),
  boostVisualFxRevert: () => callOrThrow<TweakState>("boost_visual_fx_revert"),
  ramPurge: () => callOrThrow<PurgeResult>("ram_purge"),
  /** Relaunches the app elevated (UAC prompt) on `route`; this instance exits on success. */
  relaunchElevated: (route: string) => callOrThrow<void>("relaunch_elevated", { route }),

  cleanupPreview: (ids: string[]) => callOrThrow<PreviewItem[]>("cleanup_preview", { ids }),
  cleanupConfirm: (ids: string[]) => callOrThrow<ConfirmToken>("cleanup_confirm", { ids }),
  cleanupExecute: (token: number, ids: string[]) =>
    callOrThrow<RunRecord>("cleanup_execute", { token, ids }),
  cleanupHistory: () => call<RunRecord[]>("cleanup_history"),

  settingsGet: () => call<Settings>("settings_get"),
  settingsSet: (settings: Settings) => callOrThrow<Settings>("settings_set", { settings }),
  autostartStatus: () => call<AutostartStatus>("autostart_status"),
  autostartSet: (on: boolean) => callOrThrow<AutostartStatus>("autostart_set", { on }),
  routineStatus: () => call<RoutineStatus>("routine_status"),

  systemProfile: (refresh = false) => call<Profile>("system_profile", { refresh }),
  recommendations: (refresh = false) => call<Recommendations>("recommendations", { refresh }),
  monitorStart: () => call<void>("monitor_start"),
  monitorStop: () => call<void>("monitor_stop"),
  restoreStatus: () => call<RestoreStatus>("restore_status"),
  restoreCreate: (description: string) =>
    callOrThrow<CreateOutcome>("restore_create", { description }),

  pingHost: (host: string, count: number, timeoutMs: number, intervalMs: number) =>
    callOrThrow<PingReport>("ping_host", { host, count, timeoutMs, intervalMs }),
  traceHost: (host: string, maxHops: number, probes: number, timeoutMs: number) =>
    callOrThrow<Hop[]>("trace_host", { host, maxHops, probes, timeoutMs }),

  /** Subscribe to a Rust-emitted event; no-op outside Tauri. */
  on: async <T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> => {
    if (!inTauri) return () => undefined;
    return listen<T>(event, (e) => cb(e.payload));
  },

  /** JSON over the Rust HTTP client (scoped in capabilities); plain fetch in the browser. */
  fetchJson: async <T>(url: string): Promise<T> => {
    const res = inTauri ? await tauriFetch(url) : await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return (await res.json()) as T;
  },
};
