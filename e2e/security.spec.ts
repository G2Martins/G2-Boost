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

test("antivirus tab: never weakens protection, vendor guides and the SmartScreen note", async ({
  page,
}) => {
  await open(page, "security", t["security.title"], "antivirus");
  await expect(page.getByRole("tab", { name: t["av.tab.antivirus"] })).toHaveAttribute(
    "aria-selected",
    "true",
  );
  await expect(page.getByText(t["av.intro"])).toBeVisible();
  await expect(page.getByText(t["av.smartscreen.title"])).toBeVisible();
  // Browser mode: no product list and no exe path, so there is nothing to copy.
  await expect(page.getByRole("button", { name: t["av.guide.copy"] })).toBeDisabled();
  await page.getByRole("combobox").selectOption("eset");
  await expect(page.getByText(t["av.guide.eset.2"])).toBeVisible();
  await page.getByRole("tab", { name: t["av.tab.exposure"] }).click();
  await expect(page.getByText(t["security.disclaimer"])).toBeVisible();
});
