import { invoke } from "@tauri-apps/api/core";

/** True inside the Tauri webview; false in a plain browser (`bun run dev`). */
export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export type AppInfo = {
  version: string;
  core_version: string;
  os: string;
  elevated: boolean;
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

export const backend = {
  appInfo: () => call<AppInfo>("app_info"),
  listDrives: () => call<DriveInfo[]>("list_drives"),
  memoryStatus: () => call<MemoryStatus>("memory_status"),
};
