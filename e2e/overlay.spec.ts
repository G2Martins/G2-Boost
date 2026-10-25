import { expect, open, t, test } from "./fixtures";

test("overlay settings render without a backend and the preview follows the controls", async ({
  page,
}) => {
  await open(page, "overlay", t["nav.overlay"]);
  await expect(page.getByText(t["overlay.preview.sample"])).toBeVisible();
  const hud = page.locator(".hud");
  await expect(hud).toHaveAttribute("data-style", "bar");
  await expect(hud).toContainText(t["overlay.hud.fps"]);
  await expect(hud).not.toContainText(t["overlay.sample.game"]);

  // style
  await page.getByRole("button", { name: t["overlay.style.list"] }).click();
  await expect(hud).toHaveAttribute("data-style", "list");

  // position: the corners of the preview are the picker
  const corner = page.getByRole("button", { name: t["overlay.corner.bottom_right"] });
  await expect(corner).toHaveAttribute("aria-pressed", "false");
  await corner.click();
  await expect(corner).toHaveAttribute("aria-pressed", "true");
  await expect(corner.locator(".hud")).toBeVisible();

  // metrics
  const ram = page.getByRole("button", { name: t["overlay.metric.ram"], exact: true });
  await expect(ram).toHaveAttribute("aria-pressed", "true");
  await ram.click();
  await expect(ram).toHaveAttribute("aria-pressed", "false");
  await expect(hud).not.toContainText(t["overlay.hud.ram"]);
  await page.getByRole("button", { name: t["overlay.metric.game"] }).click();
  await expect(hud).toContainText(t["overlay.sample.game"]);

  // hotkey picker (safe list) and sliders
  await page.getByRole("button", { name: "Ctrl", exact: true }).click();
  await expect(page.getByText(t["overlay.hotkey.off"].replace("{key}", "Ctrl + F7"))).toBeVisible();
  await page.getByLabel(t["overlay.hotkey.key"]).selectOption("F8");
  await expect(page.getByText(t["overlay.hotkey.off"].replace("{key}", "Ctrl + F8"))).toBeVisible();
  await page.getByLabel(t["overlay.opacity"]).fill("40");
  await expect(page.getByText("40 %")).toBeVisible();

  // honest notes; "Test now" only works inside the app
  await expect(page.getByText(t["overlay.how.anticheat"])).toBeVisible();
  await expect(page.getByText(t["overlay.how.admin"])).toBeVisible();
  await expect(page.getByRole("button", { name: t["overlay.test"] })).toBeDisabled();
});
