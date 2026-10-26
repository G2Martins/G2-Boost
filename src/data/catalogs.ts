import {
  Accessibility,
  Activity,
  AppWindow,
  AudioWaveform,
  BellOff,
  Blend,
  BotOff,
  BrainCircuit,
  BriefcaseBusiness,
  BugOff,
  CircleUserRound,
  Cpu,
  EarOff,
  FileSearch,
  FileType,
  Gamepad2,
  Gauge,
  Globe,
  Gpu,
  Grid2x2,
  HardDrive,
  ImageOff,
  Joystick,
  Keyboard,
  Languages,
  Lightbulb,
  type LucideIcon,
  MapPinOff,
  Megaphone,
  MemoryStick,
  MessageCircleOff,
  MessageSquareWarning,
  Monitor,
  MonitorCog,
  Mouse,
  MousePointer2,
  Network,
  Newspaper,
  NotebookPen,
  PackageMinus,
  PackageX,
  Paintbrush,
  PanelBottom,
  PanelsTopLeft,
  Plug,
  PowerOff,
  Printer,
  Radar,
  RadioTower,
  Recycle,
  Rocket,
  Satellite,
  ScanEye,
  Search,
  SearchX,
  Share2,
  ShieldOff,
  ShoppingBag,
  SignalLow,
  SquareDashedMousePointer,
  SquareMenu,
  Stethoscope,
  Wifi,
} from "lucide-react";
import type { TweakCategory } from "@/lib/backend";
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
  {
    id: "firefox_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["Firefox"],
    defaultOn: true,
  },
  { id: "brave_cache", group: "monthly", risk: "low", requiresClosed: ["Brave"], defaultOn: true },
  { id: "opera_cache", group: "monthly", risk: "low", requiresClosed: ["Opera"], defaultOn: true },
  {
    id: "vivaldi_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["Vivaldi"],
    defaultOn: true,
  },
  { id: "steam_cache", group: "monthly", risk: "low", requiresClosed: ["Steam"], defaultOn: true },
  {
    id: "epic_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["Epic Games"],
    defaultOn: true,
  },
  {
    id: "battlenet_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["Battle.net"],
    defaultOn: true,
  },
  { id: "ea_cache", group: "monthly", risk: "low", requiresClosed: ["EA app"], defaultOn: true },
  {
    id: "ubisoft_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["Ubisoft Connect"],
    defaultOn: true,
  },
  { id: "teams_cache", group: "monthly", risk: "low", requiresClosed: ["Teams"], defaultOn: true },
  { id: "slack_cache", group: "monthly", risk: "low", requiresClosed: ["Slack"], defaultOn: true },
  {
    id: "vscode_cache",
    group: "monthly",
    risk: "low",
    requiresClosed: ["VS Code"],
    defaultOn: true,
  },
  { id: "dism", group: "monthly", risk: "medium", admin: true, defaultOn: false },
  { id: "logs_windows", group: "advanced", risk: "medium", admin: true, defaultOn: false },
  { id: "dumps", group: "advanced", risk: "medium", admin: true, defaultOn: false },
  { id: "dev_npm", group: "advanced", risk: "medium", defaultOn: false },
  { id: "dev_pip", group: "advanced", risk: "medium", defaultOn: false },
  { id: "dev_nuget", group: "advanced", risk: "medium", defaultOn: false },
  { id: "dev_yarn", group: "advanced", risk: "medium", defaultOn: false },
];

export const cleanupName = (id: string) => `cleanup.cat.${id}` as I18nKey;
export const cleanupHint = (id: string) => `cleanup.cat.${id}.hint` as I18nKey;

/* ---------- Boost tweaks. Mechanics (registry keys, services, SPI, power, DISM) live in the Rust
   core (crates/core/src/boost.rs); this list decides presentation, preset membership and the
   Debloat page section. A core test checks that both lists, the category and the admin flag
   agree, and that every id has name/desc/warn in both dictionaries.
   Presets (owner's decision 2026-10-14): "default" = safe set + SysMain/telemetry off;
   "high" = everything non-beta; "custom" = the user toggles each card. A preset only manages
   the tweaks listed in some preset: the rest (debloat, Windows tweaks) stay as the user set them. ---------- */
export type Preset = "default" | "high" | "custom";
export type DebloatLevel = "essential" | "recommended" | "advanced";
export const DEBLOAT_LEVELS: DebloatLevel[] = ["essential", "recommended", "advanced"];
/** What it takes for the change to show: restarting the PC, signing out and in, or Explorer. */
export type Restart = "reboot" | "signout" | "explorer";
export const TWEAK_CATEGORIES: TweakCategory[] = [
  "performance",
  "gaming",
  "privacy",
  "ai",
  "appearance",
  "network",
  "system",
];
export type Tweak = {
  id: string;
  icon: LucideIcon;
  category: TweakCategory;
  beta?: boolean;
  admin?: boolean;
  restart?: Restart;
  /** Shown on the Debloat page, in this section. */
  level?: DebloatLevel;
  presets: Exclude<Preset, "custom">[];
};
export const TWEAKS: Tweak[] = [
  { id: "visual_fx", icon: Monitor, category: "appearance", presets: ["default", "high"] },
  { id: "game_mode", icon: Gamepad2, category: "gaming", presets: ["default", "high"] },
  { id: "input_lag", icon: MousePointer2, category: "gaming", presets: ["default", "high"] },
  {
    id: "windowed_optimizations",
    icon: AppWindow,
    category: "gaming",
    presets: ["default", "high"],
  },
  { id: "keyboard", icon: Keyboard, category: "gaming", presets: ["default", "high"] },
  { id: "mouse", icon: Mouse, category: "gaming", presets: ["high"] },
  {
    id: "tips",
    icon: Lightbulb,
    category: "privacy",
    level: "essential",
    presets: ["default", "high"],
  },
  { id: "show_extensions", icon: FileType, category: "appearance", presets: ["default", "high"] },
  {
    id: "widgets_off",
    icon: Newspaper,
    category: "appearance",
    level: "essential",
    presets: ["default", "high"],
  },
  {
    id: "search_highlights_off",
    icon: Search,
    category: "privacy",
    level: "essential",
    presets: ["default", "high"],
  },
  {
    id: "silent_apps_off",
    icon: PackageX,
    category: "privacy",
    level: "essential",
    presets: ["default", "high"],
  },
  {
    id: "advertising_id_off",
    icon: Megaphone,
    category: "privacy",
    level: "essential",
    presets: ["default", "high"],
  },
  {
    id: "compat_telemetry_off",
    icon: Radar,
    category: "privacy",
    admin: true,
    level: "recommended",
    presets: ["default", "high"],
  },
  {
    id: "svc_telemetry",
    icon: Satellite,
    category: "privacy",
    admin: true,
    level: "advanced",
    presets: ["default", "high"],
  },
  {
    id: "svc_sysmain",
    icon: MemoryStick,
    category: "performance",
    admin: true,
    presets: ["default", "high"],
  },
  { id: "power_plan", icon: Plug, category: "performance", admin: true, presets: ["high"] },
  { id: "hags", icon: Gpu, category: "gaming", admin: true, restart: "reboot", presets: ["high"] },
  { id: "mmcss", icon: AudioWaveform, category: "gaming", admin: true, presets: ["high"] },
  {
    id: "network_latency",
    icon: Network,
    category: "network",
    admin: true,
    restart: "reboot",
    presets: ["high"],
  },
  { id: "svc_search", icon: FileSearch, category: "performance", admin: true, presets: ["high"] },
  // Already Manual/stopped on Windows: zero gain, real side effects (Game Pass) → custom only.
  { id: "svc_xbox", icon: Joystick, category: "performance", admin: true, presets: [] },
  { id: "svc_dps", icon: Stethoscope, category: "performance", admin: true, presets: [] },
  {
    id: "svc_geolocation",
    icon: MapPinOff,
    category: "privacy",
    admin: true,
    level: "advanced",
    presets: [],
  },
  // The one lever with measured FPS (~5 %) and the one that lowers security: opt-in, reboot.
  {
    id: "hvci_off",
    icon: ShieldOff,
    category: "performance",
    admin: true,
    beta: true,
    restart: "reboot",
    presets: [],
  },
  { id: "priority_sep", icon: Cpu, category: "gaming", admin: true, beta: true, presets: [] },
  {
    id: "perf_counters",
    icon: Activity,
    category: "performance",
    admin: true,
    beta: true,
    presets: [],
  },
  { id: "winsat_task_off", icon: Gauge, category: "performance", admin: true, presets: ["high"] },
  {
    id: "feedback_tasks_off",
    icon: BugOff,
    category: "privacy",
    admin: true,
    level: "recommended",
    presets: ["default", "high"],
  },
  // Turns printing off: custom only, recommended by the core just on PCs without a real printer.
  { id: "svc_spooler", icon: Printer, category: "performance", admin: true, presets: [] },
  // Round 9: debloat.
  {
    id: "edge_background_off",
    icon: Globe,
    category: "performance",
    admin: true,
    level: "essential",
    presets: [],
  },
  {
    id: "widgets_policy_off",
    icon: PanelsTopLeft,
    category: "appearance",
    admin: true,
    restart: "signout",
    level: "recommended",
    presets: [],
  },
  {
    id: "teams_chat_off",
    icon: MessageCircleOff,
    category: "appearance",
    level: "essential",
    presets: [],
  },
  {
    id: "consumer_features_off",
    icon: ShoppingBag,
    category: "privacy",
    admin: true,
    level: "essential",
    presets: [],
  },
  {
    id: "telemetry_min",
    icon: SignalLow,
    category: "privacy",
    admin: true,
    restart: "reboot",
    level: "recommended",
    presets: [],
  },
  {
    id: "diagtrack_manual",
    icon: RadioTower,
    category: "privacy",
    admin: true,
    restart: "reboot",
    level: "recommended",
    presets: [],
  },
  {
    id: "copilot_off",
    icon: BotOff,
    category: "ai",
    admin: true,
    restart: "signout",
    level: "recommended",
    presets: [],
  },
  {
    id: "recall_off",
    icon: ScanEye,
    category: "ai",
    admin: true,
    restart: "signout",
    level: "recommended",
    presets: [],
  },
  {
    id: "click_to_do_off",
    icon: SquareDashedMousePointer,
    category: "ai",
    admin: true,
    restart: "signout",
    level: "recommended",
    presets: [],
  },
  {
    id: "recall_feature_off",
    icon: PackageMinus,
    category: "ai",
    admin: true,
    restart: "reboot",
    level: "advanced",
    presets: [],
  },
  {
    id: "paint_ai_off",
    icon: Paintbrush,
    category: "ai",
    admin: true,
    level: "recommended",
    presets: [],
  },
  {
    id: "notepad_ai_off",
    icon: NotebookPen,
    category: "ai",
    admin: true,
    level: "recommended",
    presets: [],
  },
  {
    id: "edge_ai_off",
    icon: BrainCircuit,
    category: "ai",
    admin: true,
    level: "recommended",
    presets: [],
  },
  {
    id: "delivery_p2p_off",
    icon: Share2,
    category: "network",
    admin: true,
    level: "recommended",
    presets: [],
  },
  {
    id: "start_web_search_off",
    icon: SearchX,
    category: "privacy",
    admin: true,
    restart: "explorer",
    level: "recommended",
    presets: [],
  },
  {
    id: "account_notifications_off",
    icon: CircleUserRound,
    category: "privacy",
    restart: "explorer",
    level: "essential",
    presets: [],
  },
  {
    id: "spotlight_off",
    icon: ImageOff,
    category: "privacy",
    admin: true,
    restart: "signout",
    level: "recommended",
    presets: [],
  },
  {
    id: "office_telemetry_off",
    icon: BriefcaseBusiness,
    category: "privacy",
    admin: true,
    level: "recommended",
    presets: [],
  },
  {
    id: "nvidia_telemetry_off",
    icon: MonitorCog,
    category: "privacy",
    admin: true,
    level: "recommended",
    presets: [],
  },
  {
    id: "error_reporting_off",
    icon: MessageSquareWarning,
    category: "privacy",
    admin: true,
    level: "advanced",
    presets: [],
  },
  // Round 9: Windows (PC Boost › Ajustes de sistema only).
  {
    id: "ntfs_last_access_off",
    icon: HardDrive,
    category: "performance",
    admin: true,
    restart: "reboot",
    presets: [],
  },
  { id: "storage_sense_off", icon: Recycle, category: "system", presets: [] },
  {
    id: "fast_startup_off",
    icon: PowerOff,
    category: "system",
    admin: true,
    restart: "reboot",
    presets: [],
  },
  {
    id: "network_throttling_off",
    icon: Rocket,
    category: "network",
    admin: true,
    restart: "reboot",
    presets: [],
  },
  { id: "wifi_power_saving_off", icon: Wifi, category: "network", admin: true, presets: [] },
  { id: "core_parking_off", icon: Grid2x2, category: "performance", admin: true, presets: [] },
  { id: "transparency_off", icon: Blend, category: "appearance", presets: [] },
  {
    id: "classic_context_menu",
    icon: SquareMenu,
    category: "appearance",
    restart: "explorer",
    presets: [],
  },
  { id: "taskbar_left", icon: PanelBottom, category: "appearance", presets: [] },
  { id: "notifications_off", icon: BellOff, category: "system", restart: "signout", presets: [] },
  { id: "alt_shift_off", icon: Languages, category: "gaming", presets: [] },
  { id: "narrator_shortcut_off", icon: EarOff, category: "gaming", presets: [] },
  { id: "accessibility_shortcuts_off", icon: Accessibility, category: "gaming", presets: [] },
];
/** "Remover a IA do Windows": one click on the Debloat page (the profiles page reuses it). */
export const AI_SET = [
  "copilot_off",
  "recall_off",
  "click_to_do_off",
  "recall_feature_off",
  "paint_ai_off",
  "notepad_ai_off",
  "edge_ai_off",
] as const;
export const tweakById = (id: string) => TWEAKS.find((tw) => tw.id === id);
/** Preset members: what applying a preset may turn on or off (everything else is left alone). */
export const PRESET_MANAGED = TWEAKS.filter((tw) => tw.presets.length > 0 || tw.id === "visual_fx");
/** Visual-effects level each preset applies. */
export const PRESET_FX_LEVEL = { default: "balanced", high: "performance" } as const;
export const FX_LEVELS = ["appearance", "minimal", "balanced", "performance"] as const;
/** i18n for the Rust-side category / "not recommended here" reason codes. */
export const categoryKey = (c: string) => `boost.category.${c}` as I18nKey;
export const reasonKey = (r: string) => `boost.reason.${r}` as I18nKey;
export const tweakName = (id: string) => `boost.tweak.${id}` as I18nKey;
export const tweakDesc = (id: string) => `boost.tweak.${id}.desc` as I18nKey;
export const tweakWarn = (id: string) => `boost.tweak.${id}.warn` as I18nKey;
