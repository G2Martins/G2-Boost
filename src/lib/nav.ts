import type { Route } from "@/components/shell/app-shell";

const EVENT = "g2boost:navigate";

/** Pages ask the shell to switch route (e.g. a KPI card's "Limpeza" button). */
export function navigate(route: Route) {
  window.dispatchEvent(new CustomEvent<Route>(EVENT, { detail: route }));
}

export function onNavigate(cb: (route: Route) => void): () => void {
  const handler = (e: Event) => cb((e as CustomEvent<Route>).detail);
  window.addEventListener(EVENT, handler);
  return () => window.removeEventListener(EVENT, handler);
}
