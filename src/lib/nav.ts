import type { Route } from "@/components/shell/app-shell";

const EVENT = "g2boost:navigate";
const DROP = "g2boost:drop";

/** Pages ask the shell to switch route (e.g. a KPI card's "Limpeza" button). */
export function navigate(route: Route) {
  window.dispatchEvent(new CustomEvent<Route>(EVENT, { detail: route }));
}

export function onNavigate(cb: (route: Route) => void): () => void {
  const handler = (e: Event) => cb((e as CustomEvent<Route>).detail);
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}

/* "Show the leftovers sweep" (SmartDelete banner): the page takes it on mount or listens. */
const OPEN_ORPHANS = "g2boost:open-orphans";
let pendingOrphans = false;

export function openOrphans() {
  pendingOrphans = true;
  window.dispatchEvent(new Event(OPEN_ORPHANS));
}

export function takeOpenOrphans(): boolean {
  const p = pendingOrphans;
  pendingOrphans = false;
  return p;
}

export function onOpenOrphans(cb: () => void): () => void {
  const handler = () => {
    pendingOrphans = false;
    cb();
  };
  window.addEventListener(OPEN_ORPHANS, handler);
  return () => window.removeEventListener(OPEN_ORPHANS, handler);
}

/** Fired by the uninstaller after a removal (the undo button refreshes). */
export const APPS_REMOVED = "g2boost:apps-removed";

/* A file dropped on the window (an .exe/.lnk → "which app is this?"). The shell emits it and
   switches to the uninstaller; the page takes the last one on mount and listens for the next. */
let lastDrop: string | null = null;

export function emitDrop(path: string) {
  lastDrop = path;
  window.dispatchEvent(new CustomEvent<string>(DROP, { detail: path }));
}

export function takeDrop(): string | null {
  const d = lastDrop;
  lastDrop = null;
  return d;
}

export function onDrop(cb: (path: string) => void): () => void {
  const handler = (e: Event) => {
    lastDrop = null;
    cb((e as CustomEvent<string>).detail);
  };
  window.addEventListener(DROP, handler);
  return () => window.removeEventListener(DROP, handler);
}
