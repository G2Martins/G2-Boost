import { expect, t, test } from "./fixtures";
import { calls, fakeTauri } from "./tauri-bridge";

const block = { kind: "confirm", action: "install", id: "Demo.App", name: "Demo", version: "1.0", token: 5 };

test("a winget confirmation is answered once: back on the page, its used token is not offered again", async ({
  page,
}) => {
  await fakeTauri(page, {
    route: "console",
    answers: {
      console_run: { ok: true, command: "winget", blocks: [block], effect: null, error: null },
      console_confirm: { ok: true, command: "winget", blocks: [], effect: null, error: null },
    },
  });
  await page.goto("/");
  const input = page.getByRole("textbox", { name: t["console.input"] });
  await input.fill("winget instalar Demo.App");
  await input.press("Enter");
  const install = page.getByRole("button", { name: t["console.winget.install"], exact: true });
  await install.click();
  await expect(install).toHaveCount(0);

  // The log outlives the page: leave and come back.
  await page.getByRole("button", { name: t["nav.dashboard"], exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: t["dash.title"] })).toBeVisible();
  await page.getByRole("button", { name: t["nav.console"], exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: t["console.title"] })).toBeVisible();
  await expect(page.getByText("Demo.App").first()).toBeVisible();
  await expect(install).toHaveCount(0);
  expect((await calls(page)).filter((c) => c.cmd === "console_confirm")).toHaveLength(1);
});
