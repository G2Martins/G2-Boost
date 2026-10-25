import { defineConfig } from "@playwright/test";

// One Vite per worktree: parallel agents set E2E_PORT so their tests never hit another tree's server.
const port = Number(process.env.E2E_PORT ?? 1420);

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
    baseURL: `http://localhost:${port}`,
    channel: "msedge",
    headless: true,
    locale: "pt-BR",
    viewport: { width: 1280, height: 820 },
  },
  webServer: {
    command: `bun run dev --port ${port}`,
    url: `http://localhost:${port}`,
    reuseExistingServer: !process.env.E2E_PORT,
    timeout: 60_000,
  },
});
