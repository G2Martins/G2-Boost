import { expect, open, t, test } from "./fixtures";

test("security page renders and says it is not an antivirus", async ({ page }) => {
  await open(page, "security", t["security.title"]);
  await expect(page.getByText(t["security.disclaimer"])).toBeVisible();
  // Browser mode: no backend, so the audit asks for the app instead of showing fake data.
  await expect(page.getByText(t["security.empty"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["security.nav"], exact: true })).toHaveAttribute(
    "aria-current",
    "page",
  );
});
