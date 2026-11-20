import { expect, open, t, test } from "./fixtures";
import { calls, emit, fakeTauri, listening } from "./tauri-bridge";

/* ---------------- Limpeza inteligente, with a fabricated plan ---------------- */

const GB = 1024 ** 3;
const drive = (free: number) => ({
  letter: "C:",
  kind: "fixed",
  total_bytes: 240 * GB,
  free_bytes: free,
});
const item = (id: string, bytes: number, extra: object = {}) => ({
  id,
  bytes,
  files: 10,
  measurable: true,
  missing: false,
  needs_admin: false,
  blocked_by: [],
  ...extra,
});

/** The owner's case: a 240 GB system SSD with 14 % free, not elevated. */
const plan = {
  at: 1,
  elevated: false,
  drive: drive(33 * GB),
  tight: true,
  safe: ["temp_user", "chrome_cache"],
  safe_bytes: 5 * GB,
  system: ["temp_windows", "sys_setup_logs"],
  system_bytes: 1 * GB,
  review: [
    { kind: "unowned", count: 4, bytes: 5 * GB, high: 1 },
    { kind: "category", id: "shaders", bytes: 2 * GB, needs_admin: false },
    { kind: "big_old", count: 2, bytes: 3 * GB },
  ],
  review_bytes: 10 * GB,
  catalog: [
    item("temp_user", 4 * GB),
    item("chrome_cache", 1 * GB, { blocked_by: ["chrome.exe"] }),
    item("temp_windows", 1 * GB, { needs_admin: true }),
    item("shaders", 2 * GB),
  ],
  handlers: [
    { id: "sys_setup_logs", bytes: 0, available: false, needs_admin: true, error: null },
  ],
  big_old: [
    { path: "C:\\Users\\tester\\Downloads\\old.iso", size: 2 * GB, mtime: 1 },
    { path: "C:\\Users\\tester\\Videos\\clip.mp4", size: 1 * GB, mtime: 1 },
  ],
  extras: [
    { id: "pagefile", bytes: 8 * GB, count: 1 },
    { id: "games", bytes: 60 * GB, count: 3 },
  ],
};

const run = {
  records: [
    {
      at: 2,
      trigger: "manual",
      duration_ms: 4200,
      items: [
        { id: "temp_user", bytes_freed: 4 * GB, files_deleted: 900, skipped: 3, error: null },
        { id: "chrome_cache", bytes_freed: 1 * GB, files_deleted: 300, skipped: 0, error: null },
      ],
    },
  ],
  before: drive(33 * GB),
  after: drive(38 * GB),
  tight: true,
};

test("analyze, three buckets, confirm, then the drive gains space and points further", async ({
  page,
}) => {
  await fakeTauri(page, {
    route: "cleanup",
    answers: {
      list_drives: [drive(33 * GB)],
      smart_analyze: plan,
      smart_confirm: { token: 7, ids: ["temp_user", "chrome_cache"] },
      smart_execute: run,
      cleanup_history: [],
      clean_app_exes: {},
    },
  });
  await open(page, "cleanup", t["cleanup.title"]);
  await page.getByRole("button", { name: t["clean.analyze"] }).click();

  // The plan: one click cleans only Safe (not elevated: System waits behind Elevate).
  await expect(page.getByRole("heading", { name: t["clean.bucket.safe"] })).toBeVisible();
  await expect(page.getByRole("heading", { name: t["clean.bucket.review"] })).toBeVisible();
  await expect(page.getByRole("heading", { name: t["clean.bucket.system"] })).toBeVisible();
  await expect(page.getByText(t["clean.review.unowned"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["clean.review.big_old.go"] })).toBeVisible();
  await expect(page.getByText(t["clean.bucket.system.unmeasured"])).toBeVisible();
  const go = page.getByRole("button", { name: /^Limpar 5 GB$/ });
  await expect(go).toBeEnabled();

  // No token before the human confirms.
  await go.click();
  expect((await calls(page)).some((c) => c.cmd === "smart_confirm")).toBe(false);
  await page.getByRole("button", { name: t["cleanup.confirm.go"] }).click();
  const sent = await calls(page);
  const confirm = sent.find((c) => c.cmd === "smart_confirm");
  const exec = sent.find((c) => c.cmd === "smart_execute");
  expect(confirm?.args.ids).toEqual(["temp_user", "chrome_cache"]);
  expect(exec?.args).toEqual({ token: 7, ids: ["temp_user", "chrome_cache"] });

  // Done: freed, before/after, still tight → see what else takes the space, games on C:.
  await expect(page.getByText(t["clean.done.lead"])).toBeVisible();
  await expect(page.getByText(/ganhou 5 GB/)).toBeVisible();
  await expect(page.getByRole("button", { name: t["clean.next.biggest"] })).toBeVisible();
  await expect(page.getByRole("button", { name: t["clean.extra.games.go"] })).toBeVisible();
});

test("the analysis shows each source live", async ({ page }) => {
  await fakeTauri(page, { route: "cleanup", answers: { list_drives: [drive(33 * GB)] } });
  await page.addInitScript(() => {
    // smart_analyze never answers here: the page stays on the live pipeline.
    const w = window as unknown as { __TAURI_INTERNALS__: { invoke: (...a: unknown[]) => unknown } };
    const inner = w.__TAURI_INTERNALS__.invoke;
    w.__TAURI_INTERNALS__.invoke = (cmd: unknown, args: unknown) =>
      cmd === "smart_analyze" ? new Promise(() => {}) : inner(cmd, args);
  });
  await open(page, "cleanup", t["cleanup.title"]);
  await page.getByRole("button", { name: t["clean.analyze"] }).click();
  await listening(page, "smart:source");
  await emit(page, "smart:source", {
    source: "catalog",
    state: "running",
    bytes: 2 * GB,
    count: 100,
    current: "chrome_cache",
  });
  await emit(page, "smart:source", {
    source: "system",
    state: "skipped",
    bytes: 0,
    count: 0,
    current: null,
  });
  const list = page.getByRole("list", { name: t["clean.sources"] });
  await expect(list.getByText(t["clean.source.now"].replace("{item}", t["cleanup.cat.chrome_cache"]))).toBeVisible();
  await expect(list.getByText(t["clean.source.system.skipped"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["cleanup.live.stop"] })).toBeEnabled();
});
