import { expect, open, t, test } from "./fixtures";

test("dashboard renders KPIs and the live monitor", async ({ page }) => {
  await open(page, "dashboard", t["dash.title"]);
  await expect(page.getByText(t["dash.kpi.score"])).toBeVisible();
  await expect(page.getByText(t["dash.monitor"], { exact: true })).toBeVisible();
  // Browser mode: no backend, so KPI numbers stay as the placeholder (never a fake 0).
  await expect(page.getByText("0%")).toHaveCount(0);
});

test("dashboard My PC tab renders", async ({ page }) => {
  await open(page, "dashboard", t["dash.title"], "mypc");
  await expect(page.getByRole("tab", { name: t["dash.tab.mypc"] })).toHaveAttribute("data-state", "active");
});
