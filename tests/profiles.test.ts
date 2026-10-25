// Pure helpers of the Profiles page: what a profile covers, the built-in sets, and the undo order.
import { expect, test } from "bun:test";
import {
  AI_SET,
  builtinEntries,
  coverage,
  currentState,
  reverseSteps,
} from "../src/components/profiles/shared";
import type { ProfileStep, ProfileStepResult, TweakState } from "../src/lib/backend";

const state = (id: string, status: TweakState["status"], extra: Partial<TweakState> = {}) =>
  ({
    id,
    admin: false,
    beta: false,
    category: "performance",
    status,
    has_backup: false,
    level: null,
    detail: null,
    services: [],
    ...extra,
  }) as TweakState;

const ok = (id: string, op: "on" | "off", good = true): ProfileStepResult => ({
  id,
  op,
  ok: good,
  error: good ? null : "needs_admin",
  state: null,
});

test("coverage counts what is on and sets aside what this build or PC lacks", () => {
  const states = {
    mouse: state("mouse", "on"),
    keyboard: state("keyboard", "off"),
    hags: state("hags", "unavailable"),
    visual_fx: state("visual_fx", "on", { level: "performance" }),
  };
  expect(coverage(["mouse", "keyboard"], null, states)).toMatchObject({ on: 1, total: 2, missing: 0 });
  // Unknown to this build and unusable on this PC both count as missing, not as "off".
  expect(coverage(["mouse", "copilot_off", "hags"], null, states)).toMatchObject({
    on: 1,
    total: 1,
    missing: 2,
  });
  // The visual effects only count when the level is the one the profile asks for (default: balanced).
  expect(coverage(["visual_fx"], "performance", states)?.on).toBe(1);
  expect(coverage(["visual_fx"], null, states)?.on).toBe(0);
  expect(coverage(["mouse"], null, null)).toBeNull();
});

test("a profile made from the current state keeps what is on and the effects level", () => {
  const states = {
    mouse: state("mouse", "on"),
    keyboard: state("keyboard", "off"),
    tips: state("tips", "partial"),
    visual_fx: state("visual_fx", "on", { level: "minimal" }),
  };
  const now = currentState(states);
  expect(now.tweaks).toContain("mouse");
  expect(now.tweaks).toContain("visual_fx");
  expect(now.tweaks).not.toContain("keyboard");
  expect(now.tweaks).not.toContain("tips");
  expect(now.fx_level).toBe("minimal");
  expect(currentState({ visual_fx: state("visual_fx", "off", { level: "appearance" }) }).fx_level).toBeNull();
});

test("the four ready-made profiles", () => {
  const t = (k: string) => k;
  const [standard, high, noAi, windows] = builtinEntries(t as never);
  expect([standard?.key, high?.key, noAi?.key, windows?.key]).toEqual(["default", "high", "no_ai", "windows"]);
  // Alto desempenho is Padrão plus more; both read the preset table of the Boost page.
  expect(standard?.ids.length).toBeGreaterThan(5);
  for (const id of standard?.ids ?? []) expect(high?.ids).toContain(id);
  expect(high?.ids.length).toBeGreaterThan(standard?.ids.length ?? 0);
  expect(standard?.fx).toBe("balanced");
  expect(high?.fx).toBe("performance");
  // Sem IA only ever turns its own set on; Padrão do Windows only turns things off.
  expect(noAi?.ids).toEqual(AI_SET);
  expect(noAi?.off).toBe("never");
  expect(windows?.ids).toEqual([]);
  expect(windows?.off).toBe("always");
});

test("undo runs the steps backwards and skips what failed", () => {
  const steps: ProfileStep[] = [
    { id: "mouse", op: "on", level: null },
    { id: "tips", op: "off", level: null },
    { id: "visual_fx", op: "off", level: "performance" },
    { id: "hags", op: "on", level: null },
    { id: "visual_fx", op: "off", level: null },
  ];
  const results = [
    ok("mouse", "on"),
    ok("tips", "off"),
    ok("visual_fx", "off"),
    ok("hags", "on", false),
    ok("visual_fx", "off"),
  ];
  expect(reverseSteps(steps, results)).toEqual([
    // (the last visual_fx step had no known level: it cannot be turned back on, so it is skipped)
    { id: "visual_fx", op: "on", level: "performance" },
    { id: "tips", op: "on", level: null },
    { id: "mouse", op: "off", level: null },
  ]);
  expect(reverseSteps([], [])).toEqual([]);
});
