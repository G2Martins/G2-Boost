import type { LucideIcon } from "lucide-react";
import { Eraser, Gauge, ShieldOff, Zap } from "lucide-react";
import { PRESET_FX_LEVEL, TWEAKS, tweakName } from "@/data/catalogs";
import type { FxLevel, ProfileStep, ProfileStepResult, TweakProfile } from "@/lib/backend";
import type { StateMap } from "@/lib/boost-hooks";
import type { I18nKey } from "@/lib/i18n";

type T = (key: I18nKey, params?: Record<string, string | number>) => string;

/** The AI set. The ids belong to the tweaks catalog; a build that lacks one skips it and says so. */
export const AI_SET = [
  "copilot_off",
  "recall_off",
  "click_to_do_off",
  "recall_feature_off",
  "paint_ai_off",
  "notepad_ai_off",
  "edge_ai_off",
];

/** `ask`: the apply dialog offers "also turn off the rest"; `never`: additive set; `always`: Windows defaults. */
export type OffMode = "ask" | "never" | "always";

export type Entry = {
  key: string;
  builtin: boolean;
  name: string;
  desc: string | null;
  icon: LucideIcon;
  ids: string[];
  fx: FxLevel | null;
  off: OffMode;
  offDefault: boolean;
};

const presetIds = (p: "default" | "high") =>
  TWEAKS.filter((tw) => tw.presets.includes(p)).map((tw) => tw.id);

/** The four ready-made profiles. Padrão and Alto desempenho follow the Boost page's preset table. */
export function builtinEntries(t: T): Entry[] {
  return [
    {
      key: "default",
      builtin: true,
      name: t("boost.preset.default"),
      desc: t("profiles.builtin.default.desc"),
      icon: Zap,
      ids: presetIds("default"),
      fx: PRESET_FX_LEVEL.default,
      off: "ask",
      offDefault: true,
    },
    {
      key: "high",
      builtin: true,
      name: t("boost.preset.high"),
      desc: t("profiles.builtin.high.desc"),
      icon: Gauge,
      ids: presetIds("high"),
      fx: PRESET_FX_LEVEL.high,
      off: "ask",
      offDefault: true,
    },
    {
      key: "no_ai",
      builtin: true,
      name: t("profiles.builtin.no_ai"),
      desc: t("profiles.builtin.no_ai.desc"),
      icon: ShieldOff,
      ids: AI_SET,
      fx: null,
      off: "never",
      offDefault: false,
    },
    {
      key: "windows",
      builtin: true,
      name: t("profiles.builtin.windows"),
      desc: t("profiles.builtin.windows.desc"),
      icon: Eraser,
      ids: [],
      fx: null,
      off: "always",
      offDefault: true,
    },
  ];
}

export type Led = { id: string; on: boolean };
export type Coverage = { leds: Led[]; on: number; total: number; missing: number };

/** How much of a profile is on right now. Ids this build lacks, or this PC cannot use, count as missing. */
export function coverage(
  ids: string[],
  fx: FxLevel | null,
  states: StateMap | null,
): Coverage | null {
  if (!states) return null;
  const leds: Led[] = [];
  let missing = 0;
  for (const id of ids) {
    const s = states[id];
    if (!s || s.status === "unavailable") {
      missing += 1;
      continue;
    }
    leds.push({ id, on: id === "visual_fx" ? s.level === (fx ?? "balanced") : s.status === "on" });
  }
  return { leds, on: leds.filter((l) => l.on).length, total: leds.length, missing };
}

/** Tweaks that are on or partly on: what "Padrão do Windows" would turn off. */
export function activeCount(states: StateMap | null): number {
  return states
    ? Object.values(states).filter((s) => s.status === "on" || s.status === "partial").length
    : 0;
}

export function tweakLabel(t: T, id: string): string {
  if (id.startsWith("periph_")) return t(`periph.setting.${id.slice(7)}` as I18nKey);
  return TWEAKS.some((x) => x.id === id) ? t(tweakName(id)) : id;
}

/** A profile made from what is on now. */
export function currentState(states: StateMap): Pick<TweakProfile, "tweaks" | "fx_level"> {
  const lvl = states.visual_fx?.level;
  return {
    tweaks: TWEAKS.filter((tw) => states[tw.id]?.status === "on").map((tw) => tw.id),
    fx_level: lvl && lvl !== "appearance" ? lvl : null,
  };
}

/** The steps that put things back, last first. Steps that failed changed nothing and are skipped. */
export function reverseSteps(steps: ProfileStep[], results: ProfileStepResult[]): ProfileStep[] {
  const back: ProfileStep[] = [];
  steps.forEach((step, i) => {
    if (!results[i]?.ok) return;
    if (step.op === "on") {
      back.push({ id: step.id, op: "off", level: null });
    } else if (step.id !== "visual_fx" || step.level) {
      back.push({ id: step.id, op: "on", level: step.level });
    }
  });
  return back.reverse();
}

export const inputClass =
  "h-9 min-w-0 rounded-md border border-border bg-background px-3 text-sm outline-none placeholder:text-muted-foreground/70 focus:border-ring disabled:opacity-50";

/** Core error codes of these pages → text. */
export function profileErrorKey(e: unknown): I18nKey | null {
  const msg = e instanceof Error ? e.message : String(e);
  if (msg === "app-only") return "profiles.error.app_only";
  const known = [
    "invalid_name",
    "name_taken",
    "too_many",
    "not_found",
    "file_too_big",
    "bad_file",
    "no_folder",
  ];
  return known.includes(msg) ? (`profiles.error.${msg}` as I18nKey) : null;
}
