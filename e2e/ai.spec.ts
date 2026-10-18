import { expect, open, t, test } from "./fixtures";

test("AI assistant renders its providers", async ({ page }) => {
  await open(page, "ai", t["ai.title"]);
  await expect(page.getByText(t["ai.provider.anthropic"], { exact: true }).first()).toBeVisible();
});
