import { expect, open, t, test } from "./fixtures";

test("packet loss shows its controls and an honest empty state outside the app", async ({ page }) => {
  await open(page, "packetloss", t["nav.packetloss"]);
  await expect(page.getByText(t["tools.loss.empty.title"])).toBeVisible();
  await expect(page.getByText(t["tools.loss.empty.body"])).toBeVisible();

  // Nothing can be probed from a browser: the hero button is there but off, and says why.
  await expect(page.getByRole("button", { name: t["tools.loss.start"] })).toBeDisabled();
  await expect(page.getByText(t["common.app_only"])).toBeVisible();

  // The router is only known inside the app. (The help bubble mentions the router too: anchor the name.)
  const option = (name: string) => page.getByRole("button", { name: new RegExp(`^${name}`) });
  const router = option(t["tools.loss.target.gateway"]);
  await expect(router).toBeDisabled();
  await expect(router).toContainText(t["tools.loss.target.gateway.none"]);
  await expect(option(t["tools.loss.target.cloudflare"])).toHaveAttribute("aria-pressed", "true");
  await expect(option(t["tools.loss.target.cloudflare"])).toContainText("1.1.1.1");
  await expect(option(t["tools.loss.target.google"])).toContainText("8.8.8.8");
});

test("the destination and the duration are picked with pressed buttons", async ({ page }) => {
  await open(page, "packetloss", t["nav.packetloss"]);
  const press = (name: string) => page.getByRole("button", { name });

  await expect(press(t["tools.loss.profile.standard"])).toHaveAttribute("aria-pressed", "true");
  await expect(press(t["tools.loss.profile.standard"])).toContainText("300");
  await press(t["tools.loss.profile.long"]).click();
  await expect(press(t["tools.loss.profile.long"])).toHaveAttribute("aria-pressed", "true");
  await expect(press(t["tools.loss.profile.long"])).toContainText("600");
  await expect(press(t["tools.loss.profile.standard"])).toHaveAttribute("aria-pressed", "false");
  await press(t["tools.loss.profile.quick"]).click();
  await expect(press(t["tools.loss.profile.quick"])).toContainText("100");

  // A typed destination appears only for "Outro".
  const input = page.getByLabel(t["tools.loss.custom.label"]);
  await expect(input).toHaveCount(0);
  await press(t["tools.loss.target.custom"]).click();
  await expect(input).toBeVisible();
  await input.fill("177.10.20.30");
  await expect(input).toHaveValue("177.10.20.30");
  await expect(input).toHaveAttribute("maxlength", "253");
  await press(t["tools.loss.target.google"]).click();
  await expect(input).toHaveCount(0);
});

test.describe("reduced motion", () => {
  test.use({ colorScheme: "dark", reducedMotion: "reduce" });

  test("everything is visible at once", async ({ page }) => {
    await open(page, "packetloss", t["nav.packetloss"]);
    const rising = page.locator(".animate-rise");
    await expect(rising.first()).toBeVisible();
    const states = await rising.evaluateAll((els) =>
      els.map((e) => ({ name: getComputedStyle(e).animationName, opacity: getComputedStyle(e).opacity })),
    );
    expect(states.length).toBeGreaterThan(1);
    for (const s of states) expect(s).toEqual({ name: "none", opacity: "1" });
  });
});
