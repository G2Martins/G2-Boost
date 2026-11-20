import { expect, open, t, test } from "./fixtures";

// Outside the app the backend answers null: every tab shows its honest empty state.

test("providers: one card per provider, no key yet, model field", async ({ page }) => {
  await open(page, "connections", t["connections.title"]);
  for (const p of ["anthropic", "openai", "gemini", "nvidia"]) {
    await expect(page.locator(`[data-provider="${p}"]`)).toBeVisible();
  }
  await expect(page.getByText(t["conn.prov.state.none"], { exact: true }).first()).toBeVisible();
  await expect(page.getByLabel(t["ai.model"]).first()).toHaveValue("claude-opus-5-5");
  // A model name the request code would refuse is flagged, not saved silently.
  await page.getByLabel(t["ai.model"]).nth(2).fill("bad/model");
  await expect(page.getByText(t["conn.prov.model_invalid"])).toBeVisible();
});

test("skills: empty state, pasting an invalid skill shows why", async ({ page }) => {
  await open(page, "connections", t["connections.title"], "skills");
  await expect(page.getByText(t["conn.skills.empty.title"])).toBeVisible();
  await expect(page.getByText(t["conn.skills.safety"])).toBeVisible();
  await page.getByRole("button", { name: t["conn.skills.paste"] }).first().click();
  const dialog = page.getByRole("dialog");
  await dialog.getByRole("textbox").fill("# just markdown, no header");
  await dialog.getByRole("button", { name: t["conn.skills.add"] }).click();
  await expect(dialog.getByRole("alert")).toHaveText(t["conn.skills.err.no_frontmatter"]);
  await dialog.getByRole("textbox").fill("---\nname: x\n---\nbody");
  await dialog.getByRole("button", { name: t["conn.skills.add"] }).click();
  await expect(dialog.getByRole("alert")).toHaveText(t["conn.skills.err.missing_description"]);
});

test("mcp: connect card and the read-only map", async ({ page }) => {
  await open(page, "connections", t["connections.title"], "mcp");
  await expect(page.getByText(t["conn.connect.title"])).toBeVisible();
  await expect(page.getByText(t["claude.client.vscode"], { exact: true })).toBeVisible();
  await expect(page.getByText(t["conn.mcp.empty.title"])).toBeVisible();
  await expect(page.getByText(t["conn.mcp.safety"])).toBeVisible();
});

test.describe("reduced motion", () => {
  test.use({ reducedMotion: "reduce" });

  test("cards are visible at once and nothing moves", async ({ page }) => {
    await open(page, "connections", t["connections.title"]);
    const rising = page.locator(".animate-rise");
    await expect(rising.first()).toBeVisible();
    const states = await rising.evaluateAll((els) =>
      els.map((e) => ({ name: getComputedStyle(e).animationName, opacity: getComputedStyle(e).opacity })),
    );
    expect(states.length).toBeGreaterThan(3);
    for (const s of states) expect(s).toEqual({ name: "none", opacity: "1" });
  });
});
