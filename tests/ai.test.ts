// Model output is untrusted: only catalog ids (no beta tweaks), capped, deduped.
import { expect, test } from "bun:test";
import {
  type Answer,
  extractActions,
  HttpError,
  isBusy,
  isQuota,
  normalizeAnswer,
  MAX_ACTIONS,
  MAX_FOLLOWUPS,
  MAX_TODO,
  systemPrompt,
  undash,
  withFallback,
  withRetry,
} from "../src/lib/ai";

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
  expect(extractActions("```g2boost-actions\n{nope\n```").actions).toEqual({
    tweaks: [],
    cleanup: [],
    startup: [],
    todo: [],
    followups: [],
    needWeb: false,
    console: [],
    remember: [],
  });
  expect(extractActions("só texto").text).toBe("só texto");
});

test("prompt embeds the context and never offers beta tweaks as actions", () => {
  const p = systemPrompt("pt-BR", '{"marker":1}');
  expect(p).toContain('{"marker":1}');
  const allowed = p.split("Tweak ids allowed:")[1]?.split("\n")[0] ?? "";
  expect(allowed).toContain("game_mode");
  expect(allowed).not.toContain("hvci_off");
});

test("to-dos and follow-ups are bounded single lines; need_web only when strictly true", () => {
  const { actions } = extractActions(
    block({
      todo: ["Ligar o XMP na BIOS", "linha\ncom quebra", "", "x".repeat(300), "a", "b", "c", "d", "e"],
      followups: ["Qual upgrade?", "Qual upgrade?", 7, "Outra?", "Mais uma?", "Sobrando?"],
      need_web: "true",
    }),
  );
  expect(actions.todo[0]).toBe("Ligar o XMP na BIOS");
  expect(actions.todo).not.toContain("linha\ncom quebra");
  expect(actions.todo.length).toBe(MAX_TODO);
  expect(actions.followups).toEqual(["Qual upgrade?", "Outra?", "Mais uma?"].slice(0, MAX_FOLLOWUPS));
  expect(actions.needWeb).toBe(false);
  expect(extractActions(block({ need_web: true })).actions.needWeb).toBe(true);
});

test("dash punctuation between words becomes a comma; ranges and empty cells stay", () => {
  expect(undash("Liga o XMP — ganho grátis")).toBe("Liga o XMP, ganho grátis");
  expect(undash("cabo – sempre")).toBe("cabo, sempre");
  expect(undash("60 – 80 FPS")).toBe("60 – 80 FPS");
  expect(undash("| — |")).toBe("| — |");
  expect(extractActions("A — B").text).toBe("A, B");
});

test("prompt: Luli, knowledge embedded, web on/off wording", () => {
  const off = systemPrompt("pt-BR", "{}", { knowledge: "<doc>KB</doc>" });
  expect(off).toContain("You are Luli");
  expect(off).toContain("\\u003cdoc>KB\\u003c/doc>");
  expect(off).toContain("Web search is OFF");
  expect(off).toContain('"need_web": true');
  expect(systemPrompt("en", "{}", { web: true })).toContain("Web search is ON");
});

test("console suggestions: bounded lines, only described when asked from the console", () => {
  const { actions } = extractActions(block({ console: ["ping 1.1.1.1", "erros 7", "x\ny", 3, "a", "b", "c", "d"] }));
  expect(actions.console).toEqual(["ping 1.1.1.1", "erros 7", "a", "b", "c"]);
  expect(systemPrompt("pt-BR", "{}")).not.toContain("CONSOLE:");
  expect(systemPrompt("pt-BR", "{}", { console: ["ping <host>"] })).toContain("ping <host>");
});

test("providers without web search never get a permission request", () => {
  const p = systemPrompt("pt-BR", "{}", { noWeb: true, web: true });
  expect(p).toContain("not available with this provider");
  expect(p).not.toContain("Web search is ON");
});

test("busy providers are retried, then Gemini falls back to the next free model; quota is not retried", async () => {
  let calls = 0;
  const ok = await withRetry(
    async () => {
      calls++;
      if (calls < 3) throw new HttpError(503, "high demand");
      return "fine";
    },
    undefined,
    [0, 0],
  );
  expect(ok).toBe("fine");
  expect(calls).toBe(3);
  // Not busy (bad key): no retry.
  let bad = 0;
  await expect(
    withRetry(
      async () => {
        bad++;
        throw new HttpError(400, "bad key");
      },
      undefined,
      [0, 0],
    ),
  ).rejects.toThrow("bad key");
  expect(bad).toBe(1);

  const answer = (text: string): Answer => ({ text, sources: [], actions: extractActions("").actions });
  const tried: string[] = [];
  const a = await withFallback(
    "m1",
    ["m1", "m2", "m3"],
    async (m) => {
      tried.push(m);
      if (m === "m1") throw new HttpError(503, "busy");
      return answer(`from ${m}`);
    },
    undefined,
    [0],
  );
  expect(a.text).toBe("from m2");
  expect(a.model).toBe("m2");
  expect(tried).toEqual(["m1", "m1", "m2"]);
  // Out of quota (429): no retry on the same model, the next model is still tried, and the
  // provider's own error reaches the UI (it used to become a generic "busy").
  const quota: string[] = [];
  const err = await withFallback(
    "x",
    ["y"],
    async (m) => {
      quota.push(m);
      throw new HttpError(429, "quota exceeded");
    },
    undefined,
    [0, 0],
  ).catch((e: unknown) => e);
  expect(quota).toEqual(["x", "y"]);
  expect(isQuota(err)).toBe(true);
  expect(isBusy(err)).toBe(false);
  expect((err as Error).message).toBe("quota exceeded");
  // Overloaded everywhere: the last 503 comes back, still "busy" for the UI.
  const busy = await withFallback(
    "x",
    ["y"],
    async () => {
      throw new HttpError(503, "high demand");
    },
    undefined,
    [],
  ).catch((e: unknown) => e);
  expect(isBusy(busy)).toBe(true);
  expect(isQuota(busy)).toBe(false);
});

test("memory and instructions reach the prompt; saved answers are re-validated", () => {
  const p = systemPrompt("pt-BR", "{}", { memory: ["Joga em 1440p"], instructions: "Respostas curtas <b>" });
  expect(p).toContain("- Joga em 1440p");
  expect(p).toContain("Respostas curtas \\u003cb>");
  expect(systemPrompt("pt-BR", "{}")).not.toContain("<memory>");
  const { actions } = extractActions(block({ remember: ["Usa monitor 144 Hz", "a\nb", 1, "x", "y", "z"] }));
  expect(actions.remember).toEqual(["Usa monitor 144 Hz", "x", "y"]);
  // A tampered history entry: unknown tweak, beta tweak and a javascript: source are dropped.
  const a = normalizeAnswer({
    text: "ok",
    sources: [{ url: "javascript:alert(1)", title: "x" }, { url: "https://www.techpowerup.com/x", title: "TPU" }],
    actions: { tweaks: ["hvci_off", "game_mode", "rm -rf"], needWeb: true, remember: ["fato"] },
  });
  expect(a?.actions.tweaks).toEqual(["game_mode"]);
  expect(a?.sources.map((s) => s.title)).toEqual(["TPU"]);
  expect(a?.actions.needWeb).toBe(true);
  expect(normalizeAnswer({ text: 1 })).toBeUndefined();
});
