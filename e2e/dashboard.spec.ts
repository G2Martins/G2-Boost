import { expect, open, t, test } from "./fixtures";

test("dashboard renders KPIs and the live monitor", async ({ page }) => {
  await open(page, "dashboard", t["dash.title"]);
  await expect(page.getByText(t["dash.kpi.score"])).toBeVisible();
  await expect(page.getByText(t["dash.monitor"], { exact: true })).toBeVisible();
  // Browser mode: no backend, so KPI numbers stay as the placeholder (never a fake 0).
  await expect(page.getByText("0%")).toHaveCount(0);
});

test("My PC page renders with its tabs", async ({ page }) => {
  await open(page, "mypc", t["nav.mypc"]);
  await expect(page.getByRole("tab", { name: t["dash.tab.overview"] })).toHaveAttribute("data-state", "active");
  await expect(page.getByRole("tab", { name: t["mypc.tab.checkup"] })).toBeVisible();
});
