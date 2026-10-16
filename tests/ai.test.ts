// Model output is untrusted: only catalog ids (no beta tweaks), capped, deduped.
import { expect, test } from "bun:test";
import { extractActions, MAX_ACTIONS, systemPrompt } from "../src/lib/ai";

const block = (o: unknown) => `Texto\n\`\`\`g2boost-actions\n${JSON.stringify(o)}\n\`\`\``;

test("drops unknown, beta, duplicate and over-limit ids", () => {
  const { text, actions } = extractActions(
    block({
      tweaks: ["game_mode", "hvci_off", "rm -rf /", "game_mode", "tips", "hags", "mmcss", "power_plan", "widgets_off", "keyboard"],
      cleanup: ["temp_user", "System32"],
      startup: ["Discord", 42, "x".repeat(300)],
    }),
  );
  expect(text).toBe("Texto");
  expect(actions.tweaks).not.toContain("hvci_off");
  expect(actions.tweaks).not.toContain("rm -rf /");
  expect(actions.tweaks.length).toBe(MAX_ACTIONS);
  expect(new Set(actions.tweaks).size).toBe(actions.tweaks.length);
  expect(actions.cleanup).toEqual(["temp_user"]);
  expect(actions.startup).toEqual(["Discord"]);
});

test("malformed or missing block gives no actions", () => {
  expect(extractActions("```g2boost-actions\n{nope\n```").actions).toEqual({ tweaks: [], cleanup: [], startup: [] });
  expect(extractActions("só texto").text).toBe("só texto");
});

test("prompt embeds the context and never offers beta tweaks as actions", () => {
  const p = systemPrompt("pt-BR", '{"marker":1}');
  expect(p).toContain('{"marker":1}');
  const allowed = p.split("Tweak ids allowed:")[1]?.split("\n")[0] ?? "";
  expect(allowed).toContain("game_mode");
  expect(allowed).not.toContain("hvci_off");
});
