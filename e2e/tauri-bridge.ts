import { expect, type Page } from "@playwright/test";

/**
 * The browser has no backend, so these specs fake the Tauri bridge (`__TAURI_INTERNALS__`) the way
 * the webview provides it: `invoke` answers from a table, `listen` records the handler and the
 * spec "emits" events by calling it. That exercises the real pages with real-looking data.
 */
export type Bridge = { route: string; tab?: string; checkup?: unknown };

export async function fakeTauri(page: Page, bridge: Bridge) {
  await page.addInitScript((b: Bridge) => {
    const w = window as unknown as Record<string, unknown>;
    let seq = 0;
    const handlers: Record<string, number[]> = {};
    const listeners = new Map<number, { event: string; handler: number }>();
    const calls: { cmd: string; args: unknown }[] = [];
    w.__calls = calls;
    w.__handlers = handlers;
    w.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener() {} };
    w.__TAURI_INTERNALS__ = {
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { windowLabel: "main", label: "main" },
      },
      transformCallback(cb: (...a: unknown[]) => unknown) {
        const id = ++seq;
        w[`_${id}`] = cb;
        return id;
      },
      async invoke(cmd: string, args: Record<string, unknown>) {
        calls.push({ cmd, args });
        if (cmd === "plugin:event|listen") {
          const id = ++seq;
          const event = String(args.event);
          listeners.set(id, { event, handler: Number(args.handler) });
          handlers[event] = [...(handlers[event] ?? []), Number(args.handler)];
          return id;
        }
        if (cmd === "plugin:event|unlisten") {
          const l = listeners.get(Number(args.eventId));
          if (l) handlers[l.event] = (handlers[l.event] ?? []).filter((h) => h !== l.handler);
          return null;
        }
        if (cmd === "app_info") {
          return {
            version: "0.1.0",
            core_version: "0.1.0",
            os: "Windows 11 (build 26200)",
            elevated: false,
            user: "tester",
            start_route: b.route,
            start_tab: b.tab ?? null,
            autorun: false,
            scan_path: null,
            exe: "G2 Boost.exe",
          };
        }
        if (cmd === "checkup_run") return b.checkup ?? null;
        if (cmd === "hw_export_csv")
          return "C:\\Users\\tester\\Documents\\G2 Boost - sensores - 2026-10-25 14-03-22.csv";
        return null;
      },
    };
  }, bridge);
}

export const emit = (page: Page, event: string, payload: unknown) =>
  page.evaluate(
    ([e, p]) => {
      const w = window as unknown as Record<string, unknown> & {
        __handlers: Record<string, number[]>;
      };
      for (const id of w.__handlers[e as string] ?? []) {
        (w[`_${id}`] as (x: unknown) => void)({ event: e, id: 0, payload: p });
      }
    },
    [event, payload] as const,
  );

export const listening = (page: Page, event: string) =>
  expect
    .poll(() =>
      page.evaluate(
        (e) =>
          (window as unknown as { __handlers: Record<string, number[]> }).__handlers[e]?.length ??
          0,
        event,
      ),
    )
    .toBeGreaterThan(0);

export const calls = (page: Page) =>
  page.evaluate(
    () =>
      (window as unknown as { __calls: { cmd: string; args: Record<string, unknown> }[] }).__calls,
  );
