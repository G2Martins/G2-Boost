import type { I18nKey } from "@/lib/i18n";

/* ---------- Cleanup catalog (Docs/references/legacy-bat-scripts.md). Paths are display-only here;
   the authoritative catalog lives in the Rust core (crates/core/src/cleanup). ---------- */
export type CleanupGroup = "daily" | "monthly" | "advanced";
export type Risk = "low" | "medium" | "high";
export type CleanupCategory = {
  id: string;
  group: CleanupGroup;
  risk: Risk;
  requiresClosed?: string[];
  admin?: boolean;
  defaultOn: boolean;
};

export const CLEANUP_CATALOG: CleanupCategory[] = [
  { id: "temp_windows", group: "daily", risk: "low", admin: true, defaultOn: true },
  { id: "temp_user", group: "daily", risk: "low", defaultOn: true },
  { id: "wer", group: "daily", risk: "low", admin: true, defaultOn: true },
  { id: "shaders", group: "daily", risk: "medium", requiresClosed: ["Jogos"], defaultOn: false },
  { id: "thumbs", group: "daily", risk: "low", requiresClosed: ["Explorer"], defaultOn: true },
  { id: "dns_flush", group: "daily", risk: "low", defaultOn: true },
  { id: "recycle", group: "daily", risk: "high", defaultOn: false },
  { id: "wu_cache", group: "monthly", risk: "medium", admin: true, defaultOn: true },
  { id: "nvidia_downloads", group: "monthly", risk: "low", admin: true, defaultOn: true },
  { id: "inetcache", group: "monthly", risk: "low", defaultOn: true },
  {
    id: "chrome_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["Chrome"],
    defaultOn: true,
  },
  { id: "edge_cache", group: "monthly", risk: "low", requiresClosed: ["Edge"], defaultOn: true },
  {
    id: "discord_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["Discord"],
    defaultOn: true,
  },
  {
    id: "spotify_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["Spotify"],
    defaultOn: false,
  },
  { id: "dism", group: "monthly", risk: "medium", admin: true, defaultOn: false },
  { id: "logs_windows", group: "advanced", risk: "medium", admin: true, defaultOn: false },
  { id: "dumps", group: "advanced", risk: "medium", admin: true, defaultOn: false },
];

export const cleanupName = (id: string) => `cleanup.cat.${id}` as I18nKey;
export const cleanupHint = (id: string) => `cleanup.cat.${id}.hint` as I18nKey;

/* ---------- Boost tweaks. Mechanics (registry keys, services, SPI) live in the Rust core
   (crates/core/src/boost.rs); this list only decides presentation and preset membership.
   Presets (owner's decision 2026-10-14): "default" = safe set + SysMain/telemetry off;
   "high" = everything non-beta; "custom" = the user toggles each card. ---------- */
export type Preset = "default" | "high" | "custom";
export type Tweak = {
  id: string;
  beta?: boolean;
  admin?: boolean;
  /** Needs a reboot to take effect. */
  reboot?: boolean;
  presets: Exclude<Preset, "custom">[];
};
export const TWEAKS: Tweak[] = [
  { id: "visual_fx", presets: ["default", "high"] },
  { id: "game_mode", presets: ["default", "high"] },
  { id: "input_lag", presets: ["default", "high"] },
  { id: "keyboard", presets: ["default", "high"] },
  { id: "mouse", presets: ["high"] },
  { id: "tips", presets: ["default", "high"] },
  { id: "show_extensions", presets: ["default", "high"] },
  { id: "widgets_off", presets: ["default", "high"] },
  { id: "search_highlights_off", presets: ["default", "high"] },
  { id: "silent_apps_off", presets: ["default", "high"] },
  { id: "advertising_id_off", presets: ["default", "high"] },
  { id: "compat_telemetry_off", admin: true, presets: ["default", "high"] },
  { id: "svc_telemetry", admin: true, presets: ["default", "high"] },
  { id: "svc_sysmain", admin: true, presets: ["default", "high"] },
  { id: "power_plan", admin: true, presets: ["high"] },
  { id: "hags", admin: true, reboot: true, presets: ["high"] },
  { id: "mmcss", admin: true, presets: ["high"] },
  { id: "network_latency", admin: true, reboot: true, presets: ["high"] },
  { id: "svc_search", admin: true, presets: ["high"] },
  // Already Manual/stopped on Windows: zero gain, real side effects (Game Pass) → custom only.
  { id: "svc_xbox", admin: true, presets: [] },
  { id: "svc_dps", admin: true, presets: [] },
  { id: "svc_geolocation", admin: true, presets: [] },
  // The one lever with measured FPS (~5 %) and the one that lowers security: opt-in, reboot.
  { id: "hvci_off", admin: true, beta: true, reboot: true, presets: [] },
  { id: "priority_sep", admin: true, beta: true, presets: [] },
  { id: "perf_counters", admin: true, beta: true, presets: [] },
];
/** Visual-effects level each preset applies. */
export const PRESET_FX_LEVEL = { default: "balanced", high: "performance" } as const;
export const FX_LEVELS = ["appearance", "minimal", "balanced", "performance"] as const;
/** i18n for the Rust-side category / "not recommended here" reason codes. */
export const categoryKey = (c: string) => `boost.category.${c}` as I18nKey;
export const reasonKey = (r: string) => `boost.reason.${r}` as I18nKey;
export const tweakName = (id: string) => `boost.tweak.${id}` as I18nKey;
export const tweakDesc = (id: string) => `boost.tweak.${id}.desc` as I18nKey;
export const tweakWarn = (id: string) => `boost.tweak.${id}.warn` as I18nKey;

/* ---------- DNS providers (same 10 ExitLag ships). ---------- */
export type DnsProvider = { id: string; name: string; primary: string; secondary: string };
export const DNS_PROVIDERS: DnsProvider[] = [
  { id: "cloudflare", name: "Cloudflare", primary: "1.1.1.1", secondary: "1.0.0.1" },
  { id: "quad9", name: "Quad9", primary: "9.9.9.9", secondary: "149.112.112.112" },
  { id: "google", name: "Google", primary: "8.8.8.8", secondary: "8.8.4.4" },
  { id: "nextdns", name: "NextDNS", primary: "45.90.28.0", secondary: "45.90.30.0" },
  { id: "opendns", name: "OpenDNS", primary: "208.67.222.222", secondary: "208.67.220.220" },
  { id: "controld", name: "Control D", primary: "76.76.2.0", secondary: "76.76.10.0" },
  { id: "adguard", name: "AdGuard", primary: "94.140.14.14", secondary: "94.140.15.15" },
  { id: "verisign", name: "Verisign", primary: "64.6.64.6", secondary: "64.6.65.6" },
  { id: "comodo", name: "Comodo", primary: "8.26.56.26", secondary: "8.20.247.20" },
  { id: "neustar", name: "Neustar", primary: "156.154.70.5", secondary: "156.154.71.5" },
];
