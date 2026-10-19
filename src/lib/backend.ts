import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { fetch as tauriFetch } from "@tauri-apps/plugin-http";

/** True inside the Tauri webview; false in a plain browser (`bun run dev`). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export type DeviceKind =
  | "router"
  | "this_pc"
  | "computer"
  | "phone"
  | "tv"
  | "console"
  | "printer"
  | "camera"
  | "speaker"
  | "iot"
  | "nas"
  | "unknown";
export type NetDevice = {
  ip: string;
  mac: string;
  vendor: string | null;
  private_mac: boolean;
  hostname: string | null;
  name: string | null;
  manufacturer: string | null;
  model: string | null;
  kind: DeviceKind;
  services: string[];
  first_seen: number;
  new: boolean;
  known: boolean;
};
export type DeviceScan = {
  subnet: string;
  swept: number;
  elapsed_ms: number;
  devices: NetDevice[];
  new_count: number;
  first_scan: boolean;
};
export type OpenPort = { port: number; service: string; risk: "high" | "medium" | "info" };

export type ConsoleCommand = { name: string; aliases: string[]; usage: string };
export type ConsoleMsg = { key: string; params: Record<string, string> };
export type ConsoleBlock =
  | { kind: "table"; columns: string[]; rows: string[][] }
  | ({ kind: "note" } & ConsoleMsg)
  | {
      kind: "confirm";
      action: "install" | "upgrade";
      id: string;
      name: string;
      version: string;
      token: number;
    };
export type ConsoleReply = {
  ok: boolean;
  command: string | null;
  blocks: ConsoleBlock[];
  effect: { kind: "help" } | { kind: "clear" } | { kind: "navigate"; route: string } | null;
  error: ConsoleMsg | null;
};

export type ClaudeClient = "desktop" | "code";
export type ClaudeTarget = {
  client: ClaudeClient;
  path: string;
  exists: boolean;
  connected: boolean;
  points_here: boolean;
};

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
  exe: string;
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
export type BigFile = { size: number; path: string; mtime: number; ids: number[] };
export type DiskSearch = { files: BigFile[]; matched: number; matched_bytes: number };
export type Essential = { id: string; path: string; size: number };
export type RecycleOutcome = { recycled: number; bytes: number; failed: number };
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
  /** Used bytes of the drive when scanning a whole drive (0 = unknown): the percentage base. */
  expected: number;
  /** The root's direct children so far, largest first (live treemap). */
  top: { name: string; is_dir: boolean; bytes: number; done: boolean }[];
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
  ai_web_always: boolean;
  luli_instructions: string;
  game_mode_games: string[];
  game_mode: GameModeOptions;
};

export type GameModeOptions = {
  power_plan: boolean;
  high_priority: boolean;
  purge_ram: boolean;
  pause_routines: boolean;
};
export type Launcher =
  | "steam"
  | "epic"
  | "ea"
  | "riot"
  | "battlenet"
  | "ubisoft"
  | "gog"
  | "rockstar"
  | "other"
  | "custom";
export type GameEntry = {
  id: string;
  name: string;
  launcher: Launcher;
  install_dir: string | null;
  main_exe: string | null;
  size_bytes: number | null;
  /** Game Mode is on for this game. */
  enabled: boolean;
  running: boolean;
};
export type GameModeSession = {
  game_id: string;
  game_name: string;
  pid: number | null;
  install_dir: string | null;
  started: number;
  prev_plan: string | null;
  /** `power_plan`, `priority`, `ram`. */
  applied: string[];
  ram_freed: number;
  /** `needs_admin`, `plan_failed`, `priority_denied`. */
  skipped: string[];
};

export type LuliGroup = { id: string; name: string };
export type LuliSummary = {
  id: string;
  title: string;
  group: string | null;
  created: number;
  updated: number;
  count: number;
};
export type LuliHistory = { groups: LuliGroup[]; conversations: LuliSummary[] };
export type LuliConversation = {
  id: string;
  title: string;
  group: string | null;
  created: number;
  updated: number;
  messages: unknown[];
};
export type LuliMemoryItem = { id: string; text: string; created: number };

export type Gpu = {
  name: string;
  vram_bytes: number;
  vendor: "nvidia" | "amd" | "intel" | "other";
  driver: string | null;
  pcie: string | null;
  power_limit_w: number | null;
};
export type Disk = {
  index: number;
  model: string;
  bus: "nvme" | "sata" | "usb" | "sas" | "raid" | "other";
  size_bytes: number;
  ssd: boolean | null;
  system: boolean;
};
export type Bios = { vendor: string; version: string; date: string };
export type MemoryModule = {
  slot: string;
  bank: string;
  size_bytes: number;
  speed_mts: number;
  configured_mts: number;
  rated_mts: number | null;
  kind: string;
  form_factor: string;
  manufacturer: string;
  part_number: string;
};
export type CacheInfo = {
  level: number;
  kind: "data" | "instruction" | "unified";
  count: number;
  size_bytes: number;
};
export type CpuDetail = {
  vendor: string;
  family: number;
  model: number;
  stepping: number;
  flags: string[];
  microcode: string | null;
  caches: CacheInfo[];
  hyperthreading: boolean;
  virtualization: boolean;
};
export type MonitorInfo = {
  name: string;
  width: number;
  height: number;
  refresh_hz: number;
  max_refresh_hz: number;
};
export type Profile = {
  computer: string;
  os: string;
  cpu: string;
  cpu_detail: CpuDetail;
  cores_physical: number;
  cores_logical: number;
  ram_bytes: number;
  memory: MemoryModule[];
  memory_slots: number | null;
  memory_below_rated: boolean;
  uptime_secs: number;
  gpus: Gpu[];
  monitors: MonitorInfo[];
  board: string | null;
  bios: Bios | null;
  system_model: string | null;
  disks: Disk[];
  system_disk_ssd: boolean | null;
  secure_boot: boolean | null;
  tpm: string | null;
  laptop: boolean;
  elevated: boolean;
};
export type WinSat = {
  state: number;
  base: number | null;
  cpu: number | null;
  memory: number | null;
  graphics: number | null;
  d3d: number | null;
  disk: number | null;
  taken: string | null;
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
  gpu_core_mhz: number | null;
  gpu_mem_mhz: number | null;
  gpu_power_w: number | null;
  cpu_power_w: number | null;
  /** From HWiNFO / LibreHardwareMonitor when one of them runs. */
  cpu_temp_c: number | null;
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

export type FindingSeverity = "info" | "warn" | "critical";
export type FindingKind = "free_fix" | "upgrade" | "temperature" | "security" | "maintenance";
export type Finding = {
  id: string;
  severity: FindingSeverity;
  kind: FindingKind;
  params: Record<string, string>;
};
export type Tier = "integrated" | "entry" | "mainstream" | "high" | "enthusiast";
export type GamingClass = {
  tier: Tier | null;
  target: "1080p" | "1440p" | "4k" | null;
  likely_bottleneck: "cpu" | "gpu" | null;
};
export type AdvisorReport = { findings: Finding[]; gaming: GamingClass };
export type DnsAdapter = {
  guid: string;
  name: string;
  description: string;
  dns: string[];
  gateway: string | null;
  static_dns: string[];
  provider: string | null;
  link_mbps: number;
  /** The interface Windows routes internet traffic through. */
  primary: boolean;
  has_backup: boolean;
};
export type DnsRankEntry = {
  id: string;
  name: string;
  primary: string;
  secondary: string;
  median_ms: number | null;
  loss_pct: number;
};

export type StartupLocation =
  | "user_run"
  | "machine_run"
  | "machine_run32"
  | "user_folder"
  | "common_folder";
export type StartupItem = {
  name: string;
  command: string;
  location: StartupLocation;
  enabled: boolean;
  needs_admin: boolean;
  protected: boolean;
};
export type ProcGroup = {
  name: string;
  count: number;
  ram_bytes: number;
  cpu_pct: number;
  system: boolean;
};
export type Firmware = {
  boot_mode: "uefi" | "legacy" | null;
  secure_boot: boolean | null;
  tpm: string | null;
  virtualization_enabled: boolean;
  hypervisor_present: boolean;
  resizable_bar: boolean | null;
  bar1_mb: number | null;
  memory_at_rated_speed: boolean | null;
  bios_vendor: string | null;
  bios_version: string | null;
  bios_date: string | null;
  bios_age_days: number | null;
  fast_startup: boolean | null;
  hibernation: boolean | null;
};
export type PowerPlan = { guid: string; name: string; active: boolean; builtin: boolean };
export type PowerPlans = { plans: PowerPlan[]; has_battery: boolean };
export type PowerSettingId =
  | "display_off"
  | "sleep_after"
  | "hibernate_after"
  | "wake_timers"
  | "disk_off"
  | "usb_suspend"
  | "pcie_aspm"
  | "cpu_min"
  | "cpu_max"
  | "cpu_boost"
  | "core_parking_min";
export type PowerKind =
  | { type: "seconds" }
  | { type: "percent" }
  | { type: "choice"; values: number[] };
export type PowerSetting = {
  id: PowerSettingId;
  kind: PowerKind;
  ac: number | null;
  dc: number | null;
};
export type ProcInfo = {
  pid: number;
  name: string;
  path: string;
  ram_bytes: number;
  cpu_pct: number;
  /** Creation time as a decimal string (sent back with the pid to act on this exact process). */
  created: string;
  system: boolean;
  protected: "system" | "security" | "service" | "self" | null;
};

export type AppEntry = {
  id: string;
  hive: "machine" | "machine32" | "user" | "store";
  key: string;
  name: string;
  publisher: string;
  version: string;
  install_location: string | null;
  exe: string | null;
  icon_source: string | null;
  size_bytes: number | null;
  install_date: string | null;
  uninstall: string;
  msi: boolean;
  needs_admin: boolean;
  protected: "runtime" | "self" | "user" | "security" | null;
};
export type Leftover = {
  id: number;
  kind: "folder" | "file" | "shortcut" | "registry" | "service";
  path: string;
  bytes: number;
  confidence: "low" | "medium" | "high";
  reason:
    | "install_folder"
    | "package_data"
    | "shortcut_target"
    | "name_match"
    | "publisher_folder"
    | "crash_dump"
    | "service_binary_gone"
    | "companion"
    | "unowned"
    | "steam_orphan";
  needs_admin: boolean;
  /** Days since anything inside changed (folder-first sweep). */
  idle_days?: number;
  /** Saves, profiles, vaults: never preselected, extra warning. */
  precious: boolean;
};
export type UndoRecord = { at: number; app: string; files: string[]; keys: string[] };
export type UndoOutcome = { files: number; keys: number; failed: number };
export type LeftoverOutcome = {
  recycled: number;
  bytes: number;
  registry: number;
  services: number;
  failed: number;
  needs_admin: number;
};
export type OrphanApp = { app: AppEntry; count: number; bytes: number };

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
  claudeTargets: () => call<ClaudeTarget[]>("claude_targets"),
  consoleCommands: () => call<ConsoleCommand[]>("console_commands"),
  luliHistory: () => call<LuliHistory>("luli_history"),
  luliGet: (id: string) => call<LuliConversation>("luli_get", { id }),
  luliSave: (conversation: Omit<LuliConversation, "created" | "updated">) =>
    callOrThrow<LuliSummary>("luli_save", {
      conversation: { ...conversation, created: 0, updated: 0 },
    }),
  luliRename: (id: string, title: string) => callOrThrow<void>("luli_rename", { id, title }),
  luliMove: (id: string, group: string | null) => callOrThrow<void>("luli_move", { id, group }),
  luliDelete: (id: string) => callOrThrow<void>("luli_delete", { id }),
  luliClear: () => callOrThrow<void>("luli_clear"),
  luliGroupPut: (id: string, name: string) =>
    callOrThrow<LuliGroup[]>("luli_group_put", { id, name }),
  luliGroupDelete: (id: string) => callOrThrow<LuliGroup[]>("luli_group_delete", { id }),
  luliMemory: () => call<LuliMemoryItem[]>("luli_memory"),
  luliRemember: (text: string) => callOrThrow<LuliMemoryItem[]>("luli_remember", { text }),
  luliForget: (id: string) => callOrThrow<LuliMemoryItem[]>("luli_forget", { id }),
  devicesScan: () => callOrThrow<DeviceScan>("devices_scan"),
  devicesPorts: (ip: string) => callOrThrow<OpenPort[]>("devices_ports", { ip }),
  devicesKnown: (mac: string, known: boolean) => callOrThrow<void>("devices_known", { mac, known }),
  consoleRun: (line: string) => callOrThrow<ConsoleReply>("console_run", { line }),
  consoleConfirm: (token: number, action: string, id: string) =>
    callOrThrow<ConsoleReply>("console_confirm", { token, action, id }),
  claudeConnect: (client: ClaudeClient) =>
    callOrThrow<ClaudeTarget[]>("claude_connect", { client }),
  claudeDisconnect: (client: ClaudeClient) =>
    callOrThrow<ClaudeTarget[]>("claude_disconnect", { client }),
  listDrives: () => call<DriveInfo[]>("list_drives"),
  memoryStatus: () => call<MemoryStatus>("memory_status"),

  diskScanStart: (root: string) => callOrThrow<void>("disk_scan_start", { root }),
  diskScanCancel: () => call<void>("disk_scan_cancel"),
  diskSummary: () => call<ScanSummary | null>("disk_summary"),
  diskTree: (ids: number[], depth = 2, maxChildren = 80) =>
    call<NodeView | null>("disk_tree", { ids, depth, maxChildren }),
  diskSearch: (query: string, n = 200) => call<DiskSearch>("disk_search", { query, n }),
  diskEssentials: () => call<Essential[]>("disk_essentials"),
  diskReveal: (ids: number[]) => callOrThrow<void>("disk_reveal", { ids }),
  diskRecycle: (selection: number[][]) =>
    callOrThrow<RecycleOutcome>("disk_recycle", { selection }),

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
  gamesList: (refresh = false) => call<GameEntry[]>("games_list", { refresh }),
  gamesIcon: (id: string) => call<string | null>("games_icon", { id }),
  gameModeStatus: () => call<GameModeSession | null>("game_mode_status"),
  gameModeStop: () => call<GameModeSession | null>("game_mode_stop"),

  systemProfile: (refresh = false) => call<Profile>("system_profile", { refresh }),
  recommendations: (refresh = false) => call<Recommendations>("recommendations", { refresh }),
  monitorStart: () => call<void>("monitor_start"),
  monitorStop: () => call<void>("monitor_stop"),
  restoreStatus: () => call<RestoreStatus>("restore_status"),
  restoreCreate: (description: string) =>
    callOrThrow<CreateOutcome>("restore_create", { description }),
  winsatLast: () => call<WinSat>("winsat_last"),
  winsatStart: () => callOrThrow<void>("winsat_start"),
  reportSave: () => callOrThrow<string>("report_save"),
  advisorReport: () => call<AdvisorReport>("advisor_report"),
  aiContext: () => callOrThrow<unknown>("ai_context"),
  aiKeyStatus: () => call<Record<string, boolean>>("ai_key_status"),
  aiKeySet: (provider: string, key: string) => callOrThrow<void>("ai_key_set", { provider, key }),
  aiKeyDelete: (provider: string) => callOrThrow<void>("ai_key_delete", { provider }),
  aiKeyGet: (provider: string) => callOrThrow<string>("ai_key_get", { provider }),

  pingHost: (host: string, count: number, timeoutMs: number, intervalMs: number) =>
    callOrThrow<PingReport>("ping_host", { host, count, timeoutMs, intervalMs }),
  tcpingHost: (host: string, port: number, count: number, timeoutMs: number, intervalMs: number) =>
    callOrThrow<PingReport>("tcping_host", { host, port, count, timeoutMs, intervalMs }),
  dnsAdapters: () => call<DnsAdapter[]>("dns_adapters"),
  dnsRank: (samples = 6) => call<DnsRankEntry[]>("dns_rank", { samples }),
  dnsApply: (guid: string, provider: string) => callOrThrow<void>("dns_apply", { guid, provider }),
  dnsRestore: (guid: string) => callOrThrow<void>("dns_restore", { guid }),
  startupList: () => call<StartupItem[]>("startup_list"),
  startupSet: (location: StartupLocation, name: string, enabled: boolean) =>
    callOrThrow<StartupItem>("startup_set", { location, name, enabled }),
  procsTop: () => call<ProcGroup[]>("procs_top"),
  powerPlans: () => call<PowerPlans>("power_plans"),
  powerActivate: (guid: string) => callOrThrow<void>("power_activate", { guid }),
  powerCreateMax: () => callOrThrow<string>("power_create_max"),
  powerDuplicate: (guid: string, name: string) =>
    callOrThrow<string>("power_duplicate", { guid, name }),
  powerRename: (guid: string, name: string) => callOrThrow<void>("power_rename", { guid, name }),
  powerDelete: (guid: string) => callOrThrow<void>("power_delete", { guid }),
  powerSettings: (guid: string) => call<PowerSetting[]>("power_settings", { guid }),
  powerWrite: (guid: string, id: PowerSettingId, ac: number | null, dc: number | null) =>
    callOrThrow<PowerSetting>("power_write", { guid, id, ac, dc }),
  procsList: () => call<ProcInfo[]>("procs_list"),
  appsList: () => call<AppEntry[]>("apps_list"),
  appsOrphans: () => call<OrphanApp[]>("apps_orphans"),
  appsLeftovers: (id: string) => call<Leftover[]>("apps_leftovers", { id }),
  appsConfirm: (ids: number[]) => callOrThrow<ConfirmToken>("apps_confirm", { ids }),
  appsRemove: (token: number, ids: number[]) =>
    callOrThrow<LeftoverOutcome>("apps_remove", { token, ids }),
  appsUninstall: (id: string) => callOrThrow<void>("apps_uninstall", { id }),
  appsInstalled: async (id: string) => (await call<boolean>("apps_installed", { id })) ?? false,
  appsResolve: async (path: string) =>
    (await call<string | null>("apps_resolve", { path })) ?? null,
  appsUndoStatus: () => call<UndoRecord | null>("apps_undo_status"),
  appsIcon: async (id: string) => (await call<string | null>("apps_icon", { id })) ?? null,
  appsCompanions: async (id: string) => (await call<AppEntry[]>("apps_companions", { id })) ?? [],
  appsUndo: () => callOrThrow<UndoOutcome>("apps_undo"),
  appsProtect: (id: string, protect: boolean) =>
    callOrThrow<Settings>("apps_protect", { id, protect }),
  procsEnd: (pid: number, created: string) => callOrThrow<void>("procs_end", { pid, created }),
  procsReveal: (pid: number, created: string) =>
    callOrThrow<void>("procs_reveal", { pid, created }),
  firmwareInfo: () => call<Firmware>("firmware_info"),
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
