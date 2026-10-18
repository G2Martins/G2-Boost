import { defineConfig } from "@playwright/test";

/**
 * UI smoke tests against the Vite dev server in a browser (`inTauri` = false: every backend
 * call returns null, so pages render their empty/loading states). Uses the Edge already
 * installed on Windows (`channel: "msedge"`): no browser download, nothing written to C:.
 */
export default defineConfig({
  testDir: "e2e",
  timeout: 30_000,
  fullyParallel: true,
  reporter: [["list"]],
  use: {
    baseURL: "http://localhost:1420",
    channel: "msedge",
    headless: true,
    locale: "pt-BR",
    viewport: { width: 1280, height: 820 },
  },
  webServer: {
    command: "bun run dev",
    url: "http://localhost:1420",
    reuseExistingServer: true,
    timeout: 60_000,
  },
});
