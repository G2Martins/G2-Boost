import { expect, open, t, test } from "./fixtures";
import { calls, emit, fakeTauri, listening } from "./tauri-bridge";

/* ---------------- Monitor de hardware ---------------- */

const sensor = (
  id: string,
  key: string | null,
  label: string,
  value: number,
  unit: string,
  n: number | null = null,
) => ({
  id,
  label,
  key,
  n,
  value,
  unit,
});

/** A small tree: CPU (load + power), GPU (temperature) and one external board. */
const snapshot = (seq: number, load: number) => ({
  seq,
  ts: 1,
  count: 5,
  external: "hwinfo",
  cpu_temp: false,
  devices: [
    {
      id: "cpu",
      label: "Intel Core i5-10400",
      key: null,
      n: null,
      kind: "cpu",
      groups: [
        {
          id: "cpu/load",
          label: "Load",
          key: "load",
          sensors: [
            sensor("cpu/load/total", "total", "Total", load, "%"),
            sensor("cpu/load/thread0", "thread", "Thread 0", load / 2, "%", 0),
          ],
        },
        {
          id: "cpu/power",
          label: "Power",
          key: "power",
          sensors: [sensor("cpu/power/pkg", "pkg", "Package", 41.2, "W")],
        },
      ],
    },
    {
      id: "gpu0",
      label: "NVIDIA GeForce RTX 3050",
      key: null,
      n: null,
      kind: "gpu",
      groups: [
        {
          id: "gpu0/temp",
          label: "Temperatures",
          key: "temp",
          sensors: [sensor("gpu0/temp/gpu_core", "gpu_core", "GPU core", 55, "°C")],
        },
      ],
    },
    {
      id: "ext/asus",
      label: "ASUS PRIME B460M",
      key: null,
      n: null,
      kind: "external",
      groups: [
        {
          id: "ext/asus/volt",
          label: "Voltages",
          key: "volt",
          sensors: [sensor("ext/asus/volt/12v", null, "+12V", 12.096, "V")],
        },
      ],
    },
  ],
});

test.describe("hardware monitor", () => {
  test("in the browser it asks for the app instead of faking sensors", async ({ page }) => {
    await open(page, "hwmonitor", t["nav.hwmonitor"]);
    await expect(page.getByText(t["common.app_only"])).toBeVisible();
    await expect(
      page.getByRole("button", { name: t["nav.hwmonitor"], exact: true }),
    ).toHaveAttribute("aria-current", "page");
  });

  test("shows the tree, keeps min and max, folds, resets and exports", async ({ page }) => {
    await fakeTauri(page, { route: "hwmonitor" });
    await open(page, "hwmonitor", t["nav.hwmonitor"]);
    const table = page.getByRole("table", { name: t["nav.hwmonitor"] });
    // Before the first sample the table is a skeleton, and the buttons wait.
    await expect(page.getByRole("button", { name: t["hw.csv"] })).toBeDisabled();
    await listening(page, "hw:tick");
    expect((await calls(page)).some((c) => c.cmd === "hw_start")).toBe(true);

    await emit(page, "hw:tick", snapshot(1, 40));
    await expect(page.getByText(t["hw.every"])).toBeVisible();
    await expect(page.getByText("5 sensores")).toBeVisible();
    await expect(table.getByText("Intel Core i5-10400")).toBeVisible();
    await expect(table.getByText("NVIDIA GeForce RTX 3050")).toBeVisible();
    await expect(table.getByText("ASUS PRIME B460M")).toBeVisible();
    // Built-in labels are translated; the board's own label is data.
    await expect(table.getByRole("cell", { name: "Thread 0", exact: true })).toBeVisible();
    await expect(table.getByRole("cell", { name: "+12V", exact: true })).toBeVisible();
    // Value, minimum and maximum of a rail seen once are the same reading.
    await expect(table.getByText("12,096 V")).toHaveCount(3);
    await expect(page.getByText("Sensores externos: HWiNFO")).toBeVisible();
    // No CPU temperature came with the readings: the honest hint is under the CPU.
    await expect(table.getByText(t["dash.monitor.cpu_temp.na"])).toBeVisible();

    await emit(page, "hw:tick", snapshot(2, 10));
    await emit(page, "hw:tick", snapshot(3, 25));
    const total = table
      .locator("tr")
      .filter({ has: page.getByRole("cell", { name: "Total", exact: true }) });
    await expect(total.getByRole("cell").nth(1)).toContainText("25,0%");
    await expect(total.getByRole("cell").nth(2)).toHaveText("10,0%");
    await expect(total.getByRole("cell").nth(3)).toHaveText("40,0%");

    // Reset: min and max start over from what is on screen.
    await page.getByRole("button", { name: t["hw.reset"] }).click();
    await expect(total.getByRole("cell").nth(2)).toHaveText("25,0%");
    await expect(total.getByRole("cell").nth(3)).toHaveText("25,0%");

    // Fold every device; unfold one; unfold everything.
    await page.getByRole("button", { name: t["hw.collapse_all"] }).click();
    await expect(table.getByRole("cell", { name: "Thread 0", exact: true })).toHaveCount(0);
    await expect(table.getByText("Intel Core i5-10400")).toBeVisible();
    await page.getByRole("button", { name: /Intel Core i5-10400/ }).click();
    await expect(table.getByRole("cell", { name: "Thread 0", exact: true })).toBeVisible();
    await page.getByRole("button", { name: t["hw.collapse_all"] }).click();
    await page.getByRole("button", { name: t["hw.expand_all"] }).click();
    await expect(table.getByRole("cell", { name: "+12V", exact: true })).toBeVisible();
    // A group folds on its own too.
    await page.getByRole("button", { name: new RegExp(`^${t["hw.g.volt"]}`) }).click();
    await expect(table.getByRole("cell", { name: "+12V", exact: true })).toHaveCount(0);

    // CSV: the rows the page shows, with the labels it shows, go to the core; the file name is a toast.
    await page.getByRole("button", { name: t["hw.csv"] }).click();
    await expect(page.getByText("G2 Boost - sensores - 2026-10-25 14-03-22.csv")).toBeVisible();
    const csv = (await calls(page)).find((c) => c.cmd === "hw_export_csv");
    const req = csv?.args.request as {
      locale: string;
      header: string[];
      rows: Record<string, unknown>[];
    };
    expect(req.locale).toBe("pt-BR");
    expect(req.header).toEqual([
      t["hw.csv.device"],
      t["hw.csv.group"],
      t["hw.col.sensor"],
      t["hw.col.value"],
      t["hw.col.min"],
      t["hw.col.max"],
      t["hw.csv.unit"],
    ]);
    expect(req.rows).toHaveLength(5);
    expect(req.rows[0]).toEqual({
      device: "Intel Core i5-10400",
      group: t["hw.g.load"],
      sensor: t["hw.s.total"],
      value: 25,
      min: 25,
      max: 25,
      unit: "%",
    });
    expect(req.rows[1]?.sensor).toBe("Thread 0");
    expect(req.rows[4]).toMatchObject({ device: "ASUS PRIME B460M", sensor: "+12V", unit: "V" });
  });

  test("leaving the page stops the sampler", async ({ page }) => {
    await fakeTauri(page, { route: "hwmonitor" });
    await open(page, "hwmonitor", t["nav.hwmonitor"]);
    await listening(page, "hw:tick");
    await page.getByRole("button", { name: t["nav.settings"], exact: true }).click();
    await expect(page.getByRole("heading", { level: 1, name: t["settings.title"] })).toBeVisible();
    await expect
      .poll(async () => (await calls(page)).filter((c) => c.cmd === "hw_stop").length)
      .toBeGreaterThan(0);
    expect(
      await page.evaluate(
        () =>
          (window as unknown as { __handlers: Record<string, number[]> }).__handlers["hw:tick"]
            ?.length,
      ),
    ).toBe(0);
  });

  test.describe("reduced motion", () => {
    test.use({ reducedMotion: "reduce" });

    test("values still update and nothing is left animating", async ({ page }) => {
      await fakeTauri(page, { route: "hwmonitor" });
      await open(page, "hwmonitor", t["nav.hwmonitor"]);
      await listening(page, "hw:tick");
      await emit(page, "hw:tick", snapshot(1, 40));
      await emit(page, "hw:tick", snapshot(2, 60));
      await expect(page.getByText("60,0%").first()).toBeVisible();
      expect(
        await page.evaluate(
          () =>
            document
              .getAnimations()
              .filter((a) => a.effect && (a.effect as KeyframeEffect).target?.closest("table"))
              .length,
        ),
      ).toBe(0);
      const rising = page.locator(".animate-rise");
      for (const s of await rising.evaluateAll((els) =>
        els.map((e) => getComputedStyle(e).animationName),
      ))
        expect(s).toBe("none");
    });
  });
});

/* ---------------- Meu PC › Check-up ---------------- */

type Row = [id: string, state: "ok" | "attention" | "unknown", extra?: Record<string, unknown>];

const ROWS: Row[] = [
  [
    "disk_free",
    "attention",
    {
      unit: "pct",
      value: 12,
      limit: 15,
      fix: "cleanup",
      params: {
        drive: "C:",
        free_bytes: String(28 * 1024 ** 3),
        total_bytes: String(238 * 1024 ** 3),
      },
    },
  ],
  [
    "ram_use",
    "ok",
    {
      unit: "pct",
      value: 41,
      limit: 85,
      fix: "boost",
      params: { used_bytes: String(13 * 1024 ** 3), total_bytes: String(32 * 1024 ** 3) },
    },
  ],
  ["disk_health", "unknown", { unit: "count", reason: "needs_admin", fix: "mypc" }],
  [
    "antivirus",
    "ok",
    { unit: "bool", value: 1, fix: "security", params: { name: "Windows Defender" } },
  ],
  ["firewall", "ok", { unit: "bool", value: 1, fix: "security" }],
  ["uac", "attention", { unit: "bool", value: 0 }],
  ["secure_boot", "attention", { unit: "bool", value: 0, fix: "mypc" }],
  ["tpm", "ok", { unit: "bool", value: 1, fix: "mypc", params: { version: "2.0" } }],
  ["activation", "ok", { unit: "bool", value: 1 }],
  [
    "windows_update",
    "ok",
    { unit: "days", value: 9, limit: 45, fix: "apps", params: { age_days: "9" } },
  ],
  ["reboot", "ok", { unit: "bool", value: 1, fix: "apps" }],
  [
    "shutdowns",
    "attention",
    { unit: "count", value: 2, limit: 0, fix: "maintenance", params: { window: "30" } },
  ],
  [
    "crashes",
    "ok",
    { unit: "count", value: 1, limit: 5, fix: "maintenance", params: { window: "7" } },
  ],
  ["startup", "ok", { unit: "count", value: 6, limit: 10, fix: "boost" }],
  ["internet", "ok", { unit: "ms", value: 12, fix: "network" }],
  ["gpu_driver", "ok", { unit: "days", value: 38, fix: "gpu", params: { age_days: "38" } }],
  ["bios_age", "ok", { unit: "days", value: 400, fix: "mypc", params: { age_days: "400" } }],
  ["xmp", "ok", { unit: "bool", value: 1, fix: "mypc" }],
  ["rebar", "unknown", { unit: "bool", reason: "not_applicable", fix: "mypc" }],
];

const report = {
  items: ROWS.map(([id, state, extra]) => ({
    id,
    state,
    reason: null,
    value: null,
    limit: null,
    unit: "bool",
    params: {},
    fix: null,
    ...extra,
  })),
  summary: { ok: 13, attention: 4, unknown: 2 },
  took_ms: 2400,
  elevated: false,
};

test.describe("check-up", () => {
  test("in the browser the tab asks for the app", async ({ page }) => {
    await open(page, "mypc", t["nav.mypc"], "checkup");
    await expect(page.getByRole("tab", { name: t["mypc.tab.checkup"] })).toHaveAttribute(
      "data-state",
      "active",
    );
    await expect(page.getByText(t["common.app_only"])).toBeVisible();
  });

  test.describe("with a report", () => {
    test.use({ reducedMotion: "reduce" });

    test.beforeEach(async ({ page }) => {
      await fakeTauri(page, { route: "mypc", tab: "checkup", checkup: report });
      await open(page, "mypc", t["nav.mypc"], "checkup");
      await expect(page.getByText(t["checkup.hint"])).toBeVisible();
    });

    test("summary, sections and every row's sentence", async ({ page }) => {
      await expect(page.getByText("de 17 ok")).toBeVisible();
      await expect(page.getByText("13", { exact: true }).first()).toBeVisible();
      await expect(page.getByText(t["checkup.attention"].replace("{count}", "4"))).toBeVisible();
      await expect(page.getByText(t["checkup.unknown"].replace("{count}", "2"))).toBeVisible();
      await expect(page.getByText("Medido em 2,4 s")).toBeVisible();
      for (const s of ["storage", "security", "windows", "firmware"] as const) {
        await expect(
          page.getByRole("heading", { name: t[`checkup.section.${s}` as const] }),
        ).toBeVisible();
      }
      await expect(page.getByText("Só 28 GB livres de 238 GB.", { exact: false })).toBeVisible();
      await expect(page.getByText("A última atualização foi há 9 dias.")).toBeVisible();
      await expect(
        page.getByText("Desligamentos inesperados nos últimos 30 dias: 2.", { exact: false }),
      ).toBeVisible();
      await expect(page.getByText(t["checkup.unknown.not_applicable"])).toBeVisible();
      await expect(page.getByText(t["checkup.unknown.needs_admin"])).toBeVisible();
      // A row that is not an app screen says where to go in Windows itself.
      await expect(page.getByText(/Controle de Conta de Usuário/).first()).toBeVisible();
    });

    test("an attention row opens the screen that fixes it", async ({ page }) => {
      const row = page.locator("li").filter({ hasText: "Espaço livre no C:" });
      await row.getByRole("button", { name: t["checkup.fix"] }).click();
      await expect(page.getByRole("heading", { level: 1, name: t["cleanup.title"] })).toBeVisible();
    });

    test("a firmware row switches to the overview where the BIOS guide lives", async ({ page }) => {
      const row = page
        .locator("li")
        .filter({ hasText: "Secure Boot" })
        .filter({ hasText: "desligado" });
      await row.getByRole("button", { name: t["checkup.fix"] }).click();
      await expect(page.getByRole("tab", { name: t["dash.tab.overview"] })).toHaveAttribute(
        "data-state",
        "active",
      );
    });

    test("a row that cannot be read without administrator offers to elevate", async ({ page }) => {
      const row = page.locator("li").filter({ hasText: "Saúde dos discos" });
      await row.getByRole("button", { name: t["boost.elevate"] }).click();
      const call = (await calls(page)).find((c) => c.cmd === "relaunch_elevated");
      expect(call?.args).toEqual({ route: "mypc", tab: "checkup" });
    });

    test("measuring again asks the core again", async ({ page }) => {
      const before = (await calls(page)).filter((c) => c.cmd === "checkup_run").length;
      await page.getByRole("button", { name: t["checkup.run"] }).click();
      await expect
        .poll(async () => (await calls(page)).filter((c) => c.cmd === "checkup_run").length)
        .toBe(before + 1);
    });
  });
});
