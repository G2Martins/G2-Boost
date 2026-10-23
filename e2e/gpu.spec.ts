import { expect, open, t, test } from "./fixtures";

test("gpu page shows its empty state without a backend", async ({ page }) => {
  await open(page, "gpu", t["gpu.title"]);
  await expect(page.getByText(t["gpu.none.title"], { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: t["gpu.nav"], exact: true })).toHaveAttribute(
    "aria-current",
    "page",
  );
});
