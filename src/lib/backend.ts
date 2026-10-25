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
  priority_rules: PriorityRule[];
  /** By game id; changed only through `gameTweaksSet` (it also writes the GPU preference). */
  game_tweaks: Record<string, GameTweaks>;
  exposure_monitor: boolean;
  always_admin: boolean;
  /** winget ids left out of "update all". */
  update_excluded: string[];
  // r9:games settings
  /** Games added by picking their exe (full paths). Changed only through `pickExe` / `gamesRemove`. */
  game_paths: string[];
  /** The exe chosen per game (id → path). Changed only through `pickExe` / `gameExeReset`. */
  game_exe_override: Record<string, string>;

  // r9:tweaks settings

  // r9:maint settings

  // r9:hw settings

  // r9:install settings

  // r9:tools settings

  // r9:overlay settings
  /** In-game overlay; changed through `overlaySet` (it also applies the hotkey). */
  overlay: OverlaySettings;

  // r9:profiles settings
  /** Named tweak profiles; changed through `profilesSave`/`profilesDelete`/`profilesImport`. */
  profiles: TweakProfile[];

  // r9:setup settings
};

export type PriorityClass = "idle" | "below_normal" | "normal" | "above_normal" | "high";
export type PriorityRule = { exe: string; class: PriorityClass };

/* ---- network usage, Wi-Fi ---- */
export type ProcNet = {
  process: string;
  exe_path: string;
  established: number;
  public_remotes: number;
  listening_ports: number[];
  udp_endpoints: number;
  /** Bytes/s (elevated only). */
  down_bps: number | null;
  up_bps: number | null;
};
export type NetUsage = { processes: ProcNet[]; tcp: number; udp: number; bytes_available: boolean };
export type WifiBand = "ghz_2_4" | "ghz_5" | "ghz_6";
export type WifiStatus = {
  ssid: string;
  bssid: string;
  auth: string;
  cipher: string;
  signal_quality: number;
  rssi_dbm: number | null;
  channel: number | null;
  band: WifiBand | null;
  phy: string;
  rx_mbps: number;
  tx_mbps: number;
};
export type WifiNetwork = {
  ssid: string;
  bssid: string;
  rssi_dbm: number;
  signal_quality: number;
  channel: number | null;
  band: WifiBand | null;
  phy: string;
  auth: string | null;
  cipher: string | null;
  connected: boolean;
};
export type WifiIssue = {
  kind: "open_network" | "wep" | "tkip" | "wpa1" | "weak_signal" | "band24_with5_available";
  severity: "info" | "warning" | "critical";
};
export type ChannelLoad = { channel: number; networks: number; load: number };
export type WifiView = {
  status: WifiStatus | null;
  issues: WifiIssue[];
  networks: WifiNetwork[];
  channels: ChannelLoad[];
  best_channel: number | null;
  /** `location_denied`, `no_service`, `api_<code>`. */
  error: string | null;
};

/* ---- disk health ---- */
export type HealthStatus = "unknown" | "good" | "caution" | "bad";
export type NvmeHealth = {
  critical_warning: number;
  temperature_c: number | null;
  available_spare: number;
  spare_threshold: number;
  percentage_used: number;
  data_read_bytes: number;
  data_written_bytes: number;
  power_cycles: number;
  power_on_hours: number;
  unsafe_shutdowns: number;
  media_errors: number;
  error_log_entries: number;
};
export type AtaAttr = { id: number; flags: number; current: number; worst: number; raw: number };
export type DiskHealth = {
  index: number;
  model: string;
  bus: string;
  key: string;
  needs_admin: boolean;
  nvme: NvmeHealth | null;
  ata: AtaAttr[];
  status: HealthStatus;
  findings: { severity: HealthStatus; code: string; value: number }[];
  /** Worse since the last saved sample. */
  changes: { code: string; from: number; to: number }[];
};

/* ---- privacy, app usage, boot ---- */
export type AccessRecord = {
  capability: "webcam" | "microphone" | "location";
  app: string;
  path: string;
  packaged: boolean;
  last_start: number;
  last_stop: number;
  in_use: boolean;
  /** Where the program is now (icon), when it still exists. */
  icon_path: string | null;
  /** Desktop app no longer on the PC. */
  gone: boolean;
};
export type AppUsage = { id: string; last_used: number | null };
export type BootReport = {
  days: number;
  boots: { at: number; total_ms: number; main_ms: number; post_ms: number }[];
  slow: {
    kind: "application" | "driver" | "service" | "background_optimization" | "device";
    name: string;
    friendly: string;
    seen: number;
    avg_degradation_ms: number;
    last_seen: number;
  }[];
  /** Newest boot ever logged, when none in the window. */
  last_known: { at: number; total_ms: number; main_ms: number; post_ms: number } | null;
  /** Diagnostic Policy Service off: Windows stopped measuring boots. */
  dps_off: boolean;
};

/* ---- duplicates ---- */
export type DupGroup = { size: number; paths: string[]; wasted: number };
export type DupProgress = { candidates: number; hashed_bytes: number; errors: number };

export type GameModeOptions = {
  power_plan: boolean;
  high_priority: boolean;
  purge_ram: boolean;
  pause_routines: boolean;
  network_priority: boolean;
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
  /** `power_plan`, `priority`, `ram`, `qos`, `affinity`. */
  applied: string[];
  ram_freed: number;
  /** `needs_admin`, `plan_failed`, `priority_denied`, `affinity_denied`. */
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
  /** 1, 2, 3… per monitor run: the chart scrolls one step per new value. */
  seq: number;
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

/** System files: one Windows Disk Cleanup handler, measured (`volcache.rs`). */
export type VolCacheItem = {
  id: string;
  bytes: number;
  available: boolean;
  needs_admin: boolean;
  error: string | null;
};
export type RepairTool = "dism_scan" | "dism_restore" | "sfc" | "mrt_quick" | "mrt_full";
export type RepairVerdict =
  | "healthy"
  | "repairable"
  | "non_repairable"
  | "repaired"
  | "clean"
  | "threats"
  | "failed"
  | "cancelled"
  | "unknown";
export type RepairOutcome = {
  tool: RepairTool;
  at: number;
  duration_ms: number;
  verdict: RepairVerdict;
  code: number;
  repaired: number;
  unrepaired: number;
  threats: string[];
};
export type RepairStatus = {
  running: RepairTool | null;
  percent: number | null;
  mrt_present: boolean;
};
export type RepairProgress = { tool: RepairTool; percent: number | null };
/* ---- BIOS guide, per-game tweaks (round 7.1) ---- */
export type BiosVendor = "asus" | "msi" | "gigabyte" | "asrock" | "generic";
export type BiosItemId =
  | "csm"
  | "xmp"
  | "rebar"
  | "secure_boot"
  | "tpm"
  | "bios_update"
  | "virtualization";
/** `path`: the firmware's own menu labels (English, untranslated), joined with " › ". */
export type BiosGuideItem = { id: BiosItemId; path: string; optional: boolean };
export type BiosGuide = {
  vendor: BiosVendor;
  enter_key: string;
  advanced_key: string | null;
  uefi: boolean;
  items: BiosGuideItem[];
};
export type GameTweaks = {
  free_core0: boolean;
  physical_only: boolean;
  perf_cores_only: boolean;
  gpu_high_perf: boolean;
};
export type CpuSummary = {
  logical: number;
  physical: number;
  smt: boolean;
  hybrid: boolean;
  perf_logical: number;
};
export type GameTweaksInfo = { cpu: CpuSummary; gpus: number };
/* ---- Segurança › Exposição ---- */
export type ExposureSeverity = "info" | "low" | "medium" | "high";
export type FwProfile = "domain" | "private" | "public";
export type ExposureFix =
  | { kind: "rdp_disable" }
  | { kind: "nla_enable" }
  | { kind: "ra_disable" }
  | { kind: "smb1_server_disable" }
  | { kind: "firewall_on"; profile: FwProfile }
  | { kind: "upnp_remove"; protocol: string; external_port: number }
  | { kind: "winrm_stop" }
  | { kind: "task_disable"; path: string };
export type ExposureItem = {
  name: string;
  path: string | null;
  source: string | null;
  reason: string | null;
  port: number | null;
  protocol: "tcp" | "udp" | null;
  remote: string | null;
  severity: ExposureSeverity;
  fix: ExposureFix | null;
};
export type ExposureCheckId =
  | "rdp"
  | "nla"
  | "remote_assistance"
  | "winrm"
  | "smb1_server"
  | "smb1_client"
  | "firewall"
  | "upnp"
  | "remote_apps"
  | "listeners"
  | "inbound"
  | "sessions"
  | "persistence"
  | "guest"
  | "antivirus";
export type ExposureCheck = {
  id: ExposureCheckId;
  state: "ok" | "exposed" | "unknown";
  severity: ExposureSeverity;
  details: Record<string, string>;
  items: ExposureItem[];
  fix: ExposureFix | null;
};
export type ExposureReport = {
  checks: ExposureCheck[];
  elevated: boolean;
  reverts: ExposureFix[];
  took_ms: number;
};
export type ExposureAlert = {
  kind: "listener" | "inbound" | "remote_app" | "upnp";
  name: string;
  port: number | null;
  at: number;
};

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
  relaunchElevated: (route: string, tab?: string) =>
    callOrThrow<void>("relaunch_elevated", { route, tab: tab ?? null }),

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
  gameServers: (id: string) =>
    call<{ ip: string; port: number; connections: number }[]>("game_servers", { id }),
  netUsage: () => call<NetUsage>("net_usage"),
  exeIcon: (path: string) => call<string | null>("exe_icon", { path }),
  netUsageStop: () => call<void>("net_usage_stop"),
  wifiReport: (refresh = false) => call<WifiView>("wifi_report", { refresh }),
  diskHealth: () => call<DiskHealth[]>("disk_health"),
  privacyAccess: () => call<AccessRecord[]>("privacy_access"),
  appsUsage: () => call<AppUsage[]>("apps_usage"),
  bootReport: () => callOrThrow<BootReport>("boot_report"),
  dupesStart: () => callOrThrow<void>("dupes_start"),
  dupesCancel: () => call<void>("dupes_cancel"),
  dupesRecycle: (paths: string[]) => callOrThrow<RecycleOutcome>("dupes_recycle", { paths }),

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
  volcachePreview: (ids: string[]) => callOrThrow<VolCacheItem[]>("volcache_preview", { ids }),
  volcacheConfirm: (ids: string[]) => callOrThrow<ConfirmToken>("volcache_confirm", { ids }),
  volcacheExecute: (token: number, ids: string[]) =>
    callOrThrow<RunRecord>("volcache_execute", { token, ids }),
  repairRun: (tool: RepairTool) => callOrThrow<RepairOutcome>("repair_run", { tool }),
  repairCancel: () => call<void>("repair_cancel"),
  repairStatus: () => call<RepairStatus>("repair_status"),
  repairHistory: () => call<RepairOutcome[]>("repair_history"),

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

  biosGuide: () => call<BiosGuide>("bios_guide"),
  /** Restarts the PC into the firmware setup (after the UI's confirmation). */
  biosReboot: () => callOrThrow<void>("bios_reboot"),
  gameTweaksInfo: () => call<GameTweaksInfo>("game_tweaks_info"),
  gameTweaksSet: (id: string, tweaks: GameTweaks) =>
    callOrThrow<Settings>("game_tweaks_set", { id, tweaks }),
  /* GPU driver settings. `game` = a games_list id (its exe picks the profile); null = global. */
  gpuInfo: () => call<GpuOverview>("gpu_info"),
  gpuNvRead: (game: string | null) => callOrThrow<NvProfile>("gpu_nv_read", { game }),
  gpuNvApply: (game: string | null, id: NvSettingId, value: string) =>
    callOrThrow<NvProfile>("gpu_nv_apply", { game, id, value }),
  gpuNvRevert: (game: string | null, id: NvSettingId) =>
    callOrThrow<NvProfile>("gpu_nv_revert", { game, id }),
  gpuNvRecommended: (game: string | null) => callOrThrow<NvProfile>("gpu_nv_recommended", { game }),
  gpuAmdRead: () => callOrThrow<AmdGpu[]>("gpu_amd_read"),
  gpuAmdSet: (index: number, changes: AmdChange[]) =>
    callOrThrow<AmdGpu[]>("gpu_amd_set", { index, changes }),
  exposureReport: () => call<ExposureReport>("exposure_report"),
  exposureFix: (fix: ExposureFix) => callOrThrow<void>("exposure_fix", { fix }),
  exposureRevert: (fix: ExposureFix) => callOrThrow<void>("exposure_revert", { fix }),
  exposureReveal: (path: string) => callOrThrow<void>("exposure_reveal", { path }),
  exposureAlerts: () => call<ExposureAlert[]>("exposure_alerts"),
  exposureAlertsClear: () => call<void>("exposure_alerts_clear"),
  /* Updates: apps through winget (one validated upgrade per package), Windows Update via WUA. */
  updatesApps: () => callOrThrow<AppUpdate[]>("updates_apps"),
  updatesExclude: (id: string, exclude: boolean) =>
    callOrThrow<string[]>("updates_exclude", { id, exclude }),
  updatesAppsConfirm: (ids: string[]) => callOrThrow<ConfirmToken>("updates_apps_confirm", { ids }),
  updatesAppsRun: (token: number, ids: string[]) =>
    callOrThrow<UpdateBatch>("updates_apps_run", { token, ids }),
  updatesAppsCancel: () => call<void>("updates_apps_cancel"),
  updatesWuSearch: (online: boolean) => callOrThrow<WuScan>("updates_wu_search", { online }),
  updatesWuCancel: () => call<void>("updates_wu_cancel"),
  updatesWuConfirm: (ids: string[]) => callOrThrow<ConfirmToken>("updates_wu_confirm", { ids }),
  updatesWuInstall: (token: number, ids: string[]) =>
    callOrThrow<UpdateBatch>("updates_wu_install", { token, ids }),
  updatesWuStatus: () => call<WuStatus>("updates_wu_status"),
  updatesHistory: () => call<UpdateBatch[]>("updates_history"),
  /** Restarts the PC (after the UI's confirmation); refused when no restart is pending. */
  updatesReboot: () => callOrThrow<void>("updates_reboot"),
  /* Manutenção+: disk check, optimize drives, boot options, connection repair. */
  volumesList: () => call<Volume[]>("volumes_list"),
  /** Stop = repairCancel (one servicing slot for the whole app). */
  volumesRun: (op: VolumeOp, volume: string) =>
    callOrThrow<VolumeOutcome>("volumes_run", { op, volume }),
  volumesStatus: () => call<VolumeRunning | null>("volumes_status"),
  volumesHistory: () => call<VolumeOutcome[]>("volumes_history"),
  /** Restarts the PC into the advanced startup options (after the UI's confirmation). */
  bootOptionsReboot: () => callOrThrow<void>("boot_options_reboot"),
  netDhcpAdapters: () => call<DhcpAdapter[]>("net_dhcp_adapters"),
  netRenew: (index: number) => callOrThrow<string | null>("net_renew", { index }),
  netFlushDns: () => callOrThrow<void>("net_flush_dns"),
  netOpenReset: () => callOrThrow<void>("net_open_reset"),
  // r9:games
  /** "Preparar e jogar": Game Mode for this session if it is off, then the game starts as the user. */
  gameLaunch: (id: string) => callOrThrow<void>("game_launch", { id }),
  /** Windows' Open dialog for an .exe: `game` null adds a game, an id sets that game's exe. Null = cancelled. */
  pickExe: (game: string | null) => callOrThrow<Settings | null>("pick_exe", { game }),
  gameExeReset: (id: string) => callOrThrow<Settings>("game_exe_reset", { id }),
  gamesRemove: (id: string) => callOrThrow<Settings>("games_remove", { id }),

  // r9:tweaks

  // r9:maint

  // r9:hw
  /* Monitor de hardware: one snapshot of the sensor tree per second on `hw:tick` while subscribed. */
  hwStart: () => call<void>("hw_start"),
  hwStop: () => call<void>("hw_stop"),
  /** Writes the CSV to Documents and reveals it in Explorer; resolves with its path. */
  hwExportCsv: (request: HwCsvRequest) => callOrThrow<string>("hw_export_csv", { request }),
  /** Meu PC › Check-up: about twenty read-only measurements (a few seconds). */
  checkupRun: () => call<CheckupReport>("checkup_run"),

  // r9:install

  // r9:tools
  /** One ICMP burst to the end (or until `packetlossCancel`); each probe also arrives as the
   *  `packetloss:probe` event. The core validates `host`; `profile` is a closed word. */
  packetlossRun: (host: string, profile: PacketLossProfile) =>
    callOrThrow<PacketLossReport>("packetloss_run", { host, profile }),
  packetlossCancel: () => call<void>("packetloss_cancel"),
  /** Saves the last run as CSV in Documents and shows it in Explorer; returns the file name. */
  packetlossExport: (locale: string) => callOrThrow<string>("packetloss_export", { locale }),

  // r9:overlay
  overlayStatus: () => call<OverlayStatus>("overlay_status"),
  /** Saves (validated in the core) and applies: hotkey, window, position. */
  overlaySet: (config: OverlaySettings) => callOrThrow<OverlayStatus>("overlay_set", { config }),
  /** Shows the overlay over whatever is on screen (or hides it). */
  overlayTest: (on: boolean) => callOrThrow<OverlayStatus>("overlay_test", { on }),

  // r9:profiles
  profilesList: () => call<TweakProfile[]>("profiles_list"),
  profilesSave: (profile: TweakProfile, previous: string | null) =>
    callOrThrow<TweakProfile[]>("profiles_save", { profile, previous }),
  profilesDelete: (name: string) => callOrThrow<TweakProfile[]>("profiles_delete", { name }),
  /** What applying these tweaks would change right now (reads only). */
  profilesPlan: (ids: string[], fxLevel: FxLevel | null, offOthers: boolean) =>
    callOrThrow<ProfilePlan>("profiles_plan", {
      target: { ids, fx_level: fxLevel, off_others: offOthers },
    }),
  /** One step of a plan through the normal tweak flow; `source` is a profile name or `@undo`. */
  profilesStep: (step: ProfileStep, source: string) =>
    callOrThrow<ProfileStepResult>("profiles_step", { step, source }),
  profilesRestoration: () => call<Restoration>("profiles_restoration"),
  profilesHistory: () => call<HistoryEntry[]>("profiles_history"),
  profilesExport: (profile: TweakProfile) =>
    callOrThrow<ProfileExport>("profiles_export", { profile }),
  profilesFiles: () => call<ProfileFile[]>("profiles_files"),
  profilesOpenFolder: () => callOrThrow<void>("profiles_open_folder"),
  profilesImport: (source: { kind: "file"; file: string } | { kind: "text"; text: string }) =>
    callOrThrow<TweakProfile[]>("profiles_import", { source }),
  periphState: () => call<PeriphState>("periph_state"),
  periphSet: (setting: PeriphSetting, value: number) =>
    callOrThrow<PeriphState>("periph_set", { setting, value }),
  /** Back to the saved original: one setting, or all (`null`). */
  periphRevert: (setting: PeriphSetting | null) =>
    callOrThrow<PeriphState>("periph_revert", { setting }),

  // r9:setup
};

export type VolumeOp = "scan" | "spot_fix" | "analyze" | "retrim" | "defrag";
export type VolumeVerdict =
  | "healthy"
  | "fixed"
  | "needs_spot_fix"
  | "needs_offline"
  | "retry"
  | "optimized"
  | "failed"
  | "cancelled";
export type Volume = {
  /** `\\?\Volume{GUID}\`: the id every operation names. */
  id: string;
  letter: string;
  label: string;
  fs: string;
  size: number;
  free: number;
  health: "healthy" | "warning" | "unhealthy" | "unknown";
  /** What Windows itself flagged. */
  needs: "scan" | "spot_fix" | "full_repair" | null;
  media: "ssd" | "hdd" | "unknown";
  system: boolean;
  fragmentation: number | null;
  last_optimized: string | null;
  /** What the core allows on this volume (enforced there too). */
  ops: VolumeOp[];
};
export type VolumeOutcome = {
  op: VolumeOp;
  volume: string;
  letter: string;
  at: number;
  duration_ms: number;
  verdict: VolumeVerdict;
  code: number;
  output: number | null;
  fragmentation: number | null;
  prompted: boolean;
};
export type VolumeRunning = {
  op: VolumeOp;
  volume: string;
  letter: string;
  percent: number | null;
};
export type VolumeProgress = { op: VolumeOp; volume: string; percent: number | null };
export type DhcpAdapter = {
  index: number;
  name: string;
  description: string;
  ipv4: string | null;
  primary: boolean;
};

export type GpuOverview = {
  gpus: Gpu[];
  /** Which section the GPU page shows (discrete vendor first); null = none supported. */
  section: "nvidia" | "amd" | "intel" | null;
  nvidia: { version: string; branch: string } | null;
};
export type NvSettingId =
  | "low_latency"
  | "power"
  | "fps_limit"
  | "vsync"
  | "shader_cache"
  | "texture_quality"
  | "threaded";
export type NvSetting = {
  id: NvSettingId;
  /** Option key, the frame cap as a number string, or null for a value no option uses. */
  value: string | null;
  /** Raw driver dwords in hex. */
  raw: string;
  /** Option keys; empty for the frame cap. */
  options: string[];
  recommended: string | null;
  /** At the recommended value or another one that counts as good. */
  good: boolean;
  user_set: boolean;
  /** G2 Boost changed it and holds the previous value. */
  backed_up: boolean;
};
export type NvProfile = { profile: string | null; settings: NvSetting[] };
export type AmdFeatureId = "anti_lag" | "chill" | "boost" | "sharpening" | "enhanced_sync" | "frtc";
export type AmdFeature = {
  feature: AmdFeatureId;
  supported: boolean;
  enabled: boolean;
  value: number | null;
  range: [number, number] | null;
  recommended: boolean | null;
};
export type AmdGpu = { index: number; name: string; discrete: boolean; features: AmdFeature[] };
export type AmdChange = { feature: AmdFeatureId; enabled?: boolean; value?: number };

export type AppUpdate = {
  id: string;
  name: string;
  version: string;
  available: string;
  /** Why "update all" leaves it out. */
  skip: "self" | "unknown_version" | "excluded" | null;
  /** Installed for all users: the installer will likely ask for admin. */
  machine: boolean;
};
export type UpdateItem = {
  id: string;
  name: string;
  status: "running" | "ok" | "failed" | "cancelled";
  reason: string;
  code: number;
};
export type UpdateBatch = {
  at: number;
  kind: "apps" | "windows";
  items: UpdateItem[];
  reboot: boolean;
  cancelled: boolean;
};
export type AppUpdateProgress = UpdateItem & { index: number; total: number };
export type WuUpdate = {
  id: string;
  title: string;
  kb: string[];
  size: number;
  severity: string | null;
  categories: string[];
  driver: boolean;
  reboot: "never" | "always" | "can";
  downloaded: boolean;
  mandatory: boolean;
};
export type WuScan = { updates: WuUpdate[]; reboot_required: boolean; online: boolean };
export type WuHistoryEntry = {
  title: string;
  kb: string | null;
  at: number;
  operation: "install" | "uninstall";
  result: "ok" | "partial" | "failed" | "cancelled" | "in_progress";
  hresult: number;
};
export type WuStatus = { history: WuHistoryEntry[]; reboot_required: boolean };
export type WuProgress = {
  phase: "download" | "install";
  percent: number;
  current: number;
  total: number;
};

// ── r9:games types ──
/** `GameTweaks` as the core stores it now: plus high priority for this one game under Game Mode. */
export type GameOptions = GameTweaks & { high_priority: boolean };

// ── r9:tweaks types ──

// ── r9:maint types ──

// ── r9:hw types ──
export type HwDeviceKind = "cpu" | "gpu" | "memory" | "disk" | "network" | "external";
export type HwSensor = {
  /** Stable between snapshots: the page keeps min/max by it. */
  id: string;
  label: string;
  /** i18n key suffix (`hw.s.<key>`) for built-in sensors; null = show `label` as is. */
  key: string | null;
  /** Index of numbered sensors (thread 3), the `{n}` of the translated label. */
  n: number | null;
  value: number;
  /** `%`, `°C`, `MHz`, `W`, `V`, `RPM`, `B`, `B/s`, `Mb/s`, or the external tool's own. */
  unit: string;
};
export type HwGroup = { id: string; label: string; key: string | null; sensors: HwSensor[] };
export type HwDevice = {
  id: string;
  /** The machine's or the tool's own name; empty when `key` names a generic device. */
  label: string;
  key: string | null;
  n: number | null;
  kind: HwDeviceKind;
  groups: HwGroup[];
};
export type HwSnapshot = {
  seq: number;
  ts: number;
  devices: HwDevice[];
  count: number;
  /** The tool feeding the external devices; null = none running. */
  external: "hwinfo" | "libre_hardware_monitor" | null;
  /** A CPU temperature is among the readings. */
  cpu_temp: boolean;
};
export type HwCsvRow = {
  device: string;
  group: string;
  sensor: string;
  value: number | null;
  min: number | null;
  max: number | null;
  unit: string;
};
export type HwCsvRequest = { locale: string; header: string[]; rows: HwCsvRow[] };

export type CheckupState = "ok" | "attention" | "unknown";
export type CheckupReason = "unreadable" | "needs_admin" | "not_applicable" | "no_history";
export type CheckupItem = {
  id: string;
  state: CheckupState;
  reason: CheckupReason | null;
  value: number | null;
  limit: number | null;
  unit: "pct" | "bytes" | "days" | "count" | "ms" | "bool";
  /** Names ending in `_bytes` or `_days` are numbers to format; the rest is text. */
  params: Record<string, string>;
  /** Route of the screen that fixes it; null = the fix lives in Windows itself. */
  fix: string | null;
};
export type CheckupReport = {
  items: CheckupItem[];
  summary: { ok: number; attention: number; unknown: number };
  took_ms: number;
  elevated: boolean;
};

// ── r9:install types ──

// ── r9:tools types ──
export type PacketLossProfile = "quick" | "standard" | "long";
export type PacketLossSummary = {
  /** Probes with an outcome (a reply or a timeout). */
  sent: number;
  received: number;
  lost: number;
  loss_pct: number;
  late: number;
  out_of_order: number;
  avg_ms: number | null;
  min_ms: number | null;
  max_ms: number | null;
  median_ms: number | null;
  p95_ms: number | null;
  jitter_ms: number | null;
  /** Most packets lost back to back. */
  longest_loss_run: number;
};
export type PacketLossSample = {
  seq: number;
  /** null = no reply within a second. */
  rtt_ms: number | null;
  late: boolean;
  out_of_order: boolean;
  arrival: number;
};
export type PacketLossEvent = {
  seq: number;
  rtt_ms: number | null;
  late: boolean;
  out_of_order: boolean;
  /** Totals so far. */
  summary: PacketLossSummary;
};
export type PacketLossReport = {
  host: string;
  ip: string;
  profile: PacketLossProfile;
  cancelled: boolean;
  started: string;
  duration_ms: number;
  samples: PacketLossSample[];
  summary: PacketLossSummary;
};

// ── r9:overlay types ──
export type OverlayMetric =
  | "game"
  | "fps"
  | "frame_time"
  | "low1"
  | "cpu_pct"
  | "cpu_mhz"
  | "cpu_temp"
  | "gpu_pct"
  | "gpu_temp"
  | "gpu_mhz"
  | "ram"
  | "vram";
export type OverlayCorner = "top_left" | "top_right" | "bottom_left" | "bottom_right";
export type OverlaySettings = {
  enabled: boolean;
  /** Canonical form from the core's safe list, e.g. `Ctrl+Shift+F8`. */
  hotkey: string;
  corner: OverlayCorner;
  metrics: OverlayMetric[];
  style: "bar" | "list";
  /** Background plate opacity, 0–100. */
  opacity: number;
  /** Scale, 75–150 (%). */
  size: number;
};
export type FpsStats = {
  fps: number;
  frame_ms: number;
  /** Average FPS of the slowest 1 % of the frames of the last 10 s. */
  low1_fps: number;
  p99_ms: number;
  frames: number;
};
/** Event `overlay:frame`, every 500 ms while the overlay is on screen. */
export type OverlayFrame = {
  config: OverlaySettings;
  game: string | null;
  fps: FpsStats | null;
  fps_state: "ok" | "needs_admin" | "error";
  sys: MonitorSample | null;
};
export type OverlayStatus = {
  settings: OverlaySettings;
  visible: boolean;
  /** `hotkey_taken`, `invalid_hotkey` or `hotkey_error <code>`. */
  hotkey_error: string | null;
  elevated: boolean;
};

// ── r9:profiles types ──
/** A named set of tweak ids (settings.json). Unknown ids are kept and skipped when applied. */
export type TweakProfile = { name: string; tweaks: string[]; fx_level: FxLevel | null };
export type ProfileStep = { id: string; op: "on" | "off"; level: FxLevel | null };
export type ProfilePlan = {
  steps: ProfileStep[];
  /** Would need administrator and the app is not elevated. */
  blocked: ProfileStep[];
  unknown: string[];
  unavailable: string[];
  already: number;
};
export type ProfileStepResult = {
  id: string;
  op: "on" | "off";
  ok: boolean;
  error: string | null;
  state: TweakState | null;
};
export type AppliedTweak = {
  id: string;
  status: TweakStatus;
  has_backup: boolean;
  admin: boolean;
  detail: string | null;
};
export type RestoreKind =
  | "dns"
  | "gpu_driver"
  | "gpu_pref"
  | "exposure"
  | "qos"
  | "apps_undo"
  | "game_mode"
  | "peripherals";
export type RestoreOther = { kind: RestoreKind; count: number; names: string[] };
export type Restoration = { tweaks: AppliedTweak[]; others: RestoreOther[] };
export type HistoryEntry = {
  at: number;
  /** A profile name, or `@undo`, `@undo_all`, `@periph`. */
  source: string;
  /** A tweak id, or `periph_<setting>`. */
  id: string;
  on: boolean;
  ok: boolean;
  error: string | null;
};
export type ProfileFile = { file: string; name: string; tweaks: number };
export type ProfileExport = { json: string; path: string | null };
export type PeriphSetting = "mouse_speed" | "wheel_lines" | "double_click" | "kb_delay" | "kb_rate";
export type PeriphState = {
  mouse_speed: number;
  /** 4294967295 = one screen per notch. */
  wheel_lines: number;
  double_click: number;
  kb_delay: number;
  kb_rate: number;
  /** Saved originals that differ from what is set now. */
  originals: { setting: PeriphSetting; value: number }[];
};

// ── r9:setup types ──
