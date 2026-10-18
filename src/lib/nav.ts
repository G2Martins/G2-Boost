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
