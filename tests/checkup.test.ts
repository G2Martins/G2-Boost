// Check-up wording: ages, measures, sentences, and the dictionaries having every row in both languages.
import { expect, test } from "bun:test";
import type { CheckupItem } from "../src/lib/backend";
import { formatDays, formatMeasure, rowText, rowTitle, SECTIONS, type T, textParams } from "../src/lib/checkup";
import { en } from "../src/locales/en";
import { ptBR } from "../src/locales/pt-BR";

/** The provider's `t`, over a plain dictionary. */
const tFor =
  (dict: Record<string, string>): T =>
  (key, params) =>
    (dict[key] ?? key).replace(/\{(\w+)\}/g, (_, k: string) => String(params?.[k] ?? `{${k}}`));
const pt = tFor(ptBR);
const enT = tFor(en);

const item = (over: Partial<CheckupItem>): CheckupItem => ({
  id: "disk_free",
  state: "ok",
  reason: null,
  value: null,
  limit: null,
  unit: "bool",
  params: {},
  fix: null,
  ...over,
});

test("ages read in their own scale", () => {
  expect(formatDays(0, pt, "pt-BR")).toBe("0 dias");
  expect(formatDays(1, pt, "pt-BR")).toBe("1 dia");
  expect(formatDays(45, pt, "pt-BR")).toBe("45 dias");
  expect(formatDays(59, pt, "pt-BR")).toBe("59 dias");
  expect(formatDays(59.6, pt, "pt-BR")).toBe("2 meses");
  expect(formatDays(90, pt, "pt-BR")).toBe("3 meses");
  expect(formatDays(800, pt, "pt-BR")).toBe("2,2 anos");
  expect(formatDays(800, enT, "en")).toBe("2.2 years");
  expect(formatDays(-4, pt, "pt-BR")).toBe("0 dias");
});

test("each unit has its measure and yes/no rows have none", () => {
  const m = (unit: CheckupItem["unit"], value: number | null) => formatMeasure(item({ unit, value }), pt, "pt-BR");
  expect(m("pct", 39.6)).toBe("40%");
  expect(m("bytes", 1536)).toBe("1,5 KB");
  expect(m("days", 9)).toBe("9 dias");
  expect(m("count", 2.4)).toBe("2");
  expect(m("ms", 12)).toBe("12 ms");
  expect(m("bool", 1)).toBeNull();
  expect(m("pct", null)).toBeNull();
  expect(m("pct", Number.NaN)).toBeNull();
});

test("sentence parameters are formatted by name and the measure is injected", () => {
  const p = textParams(
    item({ unit: "count", value: 3, params: { free_bytes: String(10 * 1024 ** 3), age_days: "90", drive: "C:", window: "30" } }),
    pt,
    "pt-BR",
  );
  expect(p).toEqual({ free_bytes: "10 GB", age_days: "3 meses", drive: "C:", window: "30", value: "3", count: 3 });
  // A hostile number keeps its raw text instead of printing NaN.
  expect(textParams(item({ params: { x_bytes: "abc" } }), pt, "pt-BR")).toEqual({ x_bytes: "abc" });
});

test("rows read as sentences in both languages", () => {
  const full = item({
    id: "disk_free",
    state: "attention",
    unit: "pct",
    value: 5,
    limit: 15,
    params: { drive: "C:", free_bytes: String(10 * 1024 ** 3), total_bytes: String(200 * 1024 ** 3) },
  });
  expect(rowTitle(full, pt, "pt-BR")).toBe("Espaço livre no C:");
  expect(rowText(full, pt, "pt-BR")).toBe("Só 10 GB livres de 200 GB. Com pouco espaço, o Windows e os jogos ficam lentos.");
  expect(rowText(full, enT, "en")).toBe("Only 10 GB free of 200 GB. With little space, Windows and games slow down.");
  const shutdowns = item({ id: "shutdowns", state: "attention", unit: "count", value: 2, params: { window: "30" } });
  expect(rowText(shutdowns, pt, "pt-BR")).toContain("nos últimos 30 dias: 2");
  const net = item({ id: "internet", unit: "ms", value: 12, state: "ok" });
  expect(rowText(net, pt, "pt-BR")).toBe("A internet respondeu em 12 ms.");
  const unread = item({ id: "rebar", state: "unknown", reason: "not_applicable" });
  expect(rowText(unread, pt, "pt-BR")).toBe("Não se aplica a este PC.");
  expect(rowText(item({ id: "rebar", state: "unknown", reason: null }), enT, "en")).toBe("Could not be measured right now.");
});

test("every row of every section has its three texts in both dictionaries", () => {
  const ids = SECTIONS.flatMap((s) => s.ids);
  expect(ids).toHaveLength(19);
  expect(new Set(ids).size).toBe(19);
  for (const id of ids) {
    for (const part of ["title", "ok", "bad"]) {
      expect(ptBR).toHaveProperty([`checkup.${id}.${part}`]);
      expect(en).toHaveProperty([`checkup.${id}.${part}`]);
    }
  }
  for (const s of SECTIONS) {
    expect(ptBR).toHaveProperty([`checkup.section.${s.id}`]);
    expect(en).toHaveProperty([`checkup.section.${s.id}`]);
  }
});

test("no sentence uses a dash as punctuation (house style for UI text)", () => {
  for (const [lang, dict] of [["pt-BR", ptBR], ["en", en]] as const) {
    for (const [k, v] of Object.entries(dict)) {
      if (!k.startsWith("checkup.") && !k.startsWith("hw.")) continue;
      expect(/ [—–-] |--/.test(v), `${lang} ${k}: ${v}`).toBe(false);
    }
  }
});
