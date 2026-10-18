import { test as base, expect, type Page } from "@playwright/test";
import { ptBR } from "../src/locales/pt-BR";

export { expect };
export const t = ptBR;

/** Every test fails on an uncaught error or a console.error (React warnings included). */
export const test = base.extend<{ errors: string[] }>({
  errors: [
    async ({ page }, use) => {
      const errors: string[] = [];
      page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
      page.on("console", (m) => {
        if (m.type() === "error") errors.push(`console: ${m.text()}`);
      });
      await use(errors);
      expect(errors, "no errors in the console").toEqual([]);
    },
    { auto: true },
  ],
});

/** Open a page through the browser-only `#/route/tab` dev aid and wait for its title. */
export async function open(page: Page, route: string, title: string, tab?: string) {
  await page.goto(`/#/${route}${tab ? `/${tab}` : ""}`);
  await expect(page.getByRole("heading", { level: 1, name: title })).toBeVisible();
}
