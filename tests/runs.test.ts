// Atividade › Execuções: every module history folded into one timeline, and the bell's lines.
import { expect, test } from "bun:test";
import type { ActivityEntry, HistoryEntry, RunRecord, UpdateBatch } from "../src/lib/backend";
import type { I18nKey } from "../src/lib/i18n";
import { dayKey, formatDuration, lineText, normalizeRuns, type T } from "../src/lib/runs";
import { ptBR } from "../src/locales/pt-BR";

const t: T = (key: I18nKey, params) =>
  (ptBR[key] as string).replace(/\{(\w+)\}/g, (_, k: string) => String(params?.[k] ?? `{${k}}`));

const cleanup = (at: number, trigger: string, items: RunRecord["items"]): RunRecord => ({
  at,
  trigger,
  duration_ms: 1200,
  items,
});
const item = (id: string, bytes: number, error: string | null = null) => ({
  id,
  bytes_freed: bytes,
  files_deleted: 1,
  skipped: 0,
  error,
});

test("one timeline, newest first, each run pointing at its page", () => {
  const profiles: HistoryEntry[] = [
    { at: 100, source: "Jogo", id: "a", on: true, ok: true, error: null },
    { at: 130, source: "Jogo", id: "b", on: true, ok: false, error: "x" },
    { at: 400, source: "@undo", id: "a", on: false, ok: true, error: null },
  ];
  const updates: UpdateBatch[] = [
    {
      at: 300,
      kind: "windows",
      reboot: true,
      cancelled: false,
      items: [
        { id: "1", name: "KB1", status: "ok", reason: "ok", code: 0 },
        { id: "2", name: "KB2", status: "failed", reason: "x", code: 1 },
      ],
    },
  ];
  const runs = normalizeRuns({
    cleanup: [
      cleanup(500, "routine", [item("temp_user", 2048), item("logs", 0, "needs_admin")]),
      cleanup(200, "app_leftovers", [item("leftovers", 10)]),
      cleanup(50, "manual", [item("sys_temp", 5)]),
      cleanup(40, "weird<script>", [item("temp_user", 1)]),
    ],
    repair: [{ tool: "sfc", at: 250, duration_ms: 60000, verdict: "repaired", code: 0, repaired: 2, unrepaired: 0, threats: [] }],
    updates,
    profiles,
    volumes: null,
  });
  // newest first by when each ended (SFC ran a minute: it ended after the update batch);
  // cleanup records are stamped at their end by the core, so `at` steps back to the start
  expect(runs.map((r) => r.at)).toEqual([499, 400, 250, 300, 199, 130, 49, 39]);

  const [routine, undo, sfc, wu, leftovers, profile, system, unknown] = runs;
  expect(routine?.route).toBe("cleanup");
  expect(routine?.result).toEqual(["activity.result.freed_errors", { bytes: 2048, count: 1 }]);
  expect(routine?.tone).toBe("warning");
  expect(undo).toMatchObject({ route: "profiles", tab: "restore", title: ["activity.run.profile.undo"] });
  expect(wu).toMatchObject({ route: "apps", tab: "updates", tone: "warning" });
  expect(wu?.result).toEqual(["activity.result.reboot", { ok: 1, total: 2 }]);
  expect(sfc).toMatchObject({ route: "maintenance", tone: "success", ms: 60000 });
  expect(leftovers).toMatchObject({ route: "apps", tab: "orphans" });
  // two steps under a minute apart are one profile run
  expect(profile?.title).toEqual(["activity.run.profile", { name: "Jogo" }]);
  expect(profile?.result).toEqual(["activity.result.steps", { ok: 1, total: 2 }]);
  expect(system).toMatchObject({ route: "cleanup", tab: "system", title: ["activity.run.cleanup.system"] });
  // a trigger this build does not know reads as a plain cleanup, never as a key from the file
  expect(unknown?.title).toEqual(["activity.run.cleanup.manual"]);
  // every key the timeline produces exists
  for (const r of runs) {
    expect(ptBR[r.title[0]]).toBeDefined();
    expect(ptBR[r.result[0]]).toBeDefined();
  }
});

test("bell lines: sizes formatted, unknown keys generic, text stays text", () => {
  const line = (key: string, args: Record<string, string>): ActivityEntry => ({
    id: 1,
    at: 0,
    kind: "routine",
    key,
    args,
    tone: "ok",
    route: "cleanup",
    read: false,
  });
  expect(lineText(t, "pt-BR", line("activity.routine.cleanup", { bytes: "1572864" }))).toBe(
    "Rotina de limpeza terminou: 1,5 MB liberados",
  );
  expect(lineText(t, "pt-BR", line("not.a.key", {}))).toBe("Algo aconteceu em routine");
  expect(lineText(t, "pt-BR", line("activity.ram.game", { game: "<b>x</b>", bytes: "nope" }))).toBe(
    "RAM limpa ao abrir <b>x</b>: nope viraram memória livre",
  );
});

test("durations and day buckets", () => {
  expect(formatDuration(400)).toBe("<1 s");
  expect(formatDuration(42_000)).toBe("42 s");
  expect(formatDuration(3_900_000)).toBe("1 h 05 min");
  const now = new Date(2026, 10, 20, 15, 0).getTime();
  expect(dayKey(new Date(2026, 10, 20, 1, 0).getTime() / 1000, now)).toBe(0);
  expect(dayKey(new Date(2026, 10, 19, 23, 0).getTime() / 1000, now)).toBe(1);
  expect(dayKey(new Date(2026, 10, 10, 9, 0).getTime() / 1000, now)).toBe(new Date(2026, 10, 10).getTime());
});
