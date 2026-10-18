import { getCurrentWebview } from "@tauri-apps/api/webview";
import { useEffect, useState } from "react";
import { AppShell, type Route } from "@/components/shell/app-shell";
import { TooltipProvider } from "@/components/ui/primitives";
import { backend, inTauri } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { I18nProvider } from "@/lib/i18n";
import { emitDrop, onNavigate } from "@/lib/nav";
import { AiPage } from "@/pages/ai";
import { AppsPage } from "@/pages/apps";
import { BoostPage } from "@/pages/boost";
import { CleanupPage } from "@/pages/cleanup";
import { DashboardPage } from "@/pages/dashboard";
import { DiskPage } from "@/pages/disk";
import { NetworkPage } from "@/pages/network";
import { SettingsPage } from "@/pages/settings";

const PAGES: Record<Route, () => React.JSX.Element> = {
  dashboard: DashboardPage,
  disk: DiskPage,
  cleanup: CleanupPage,
  apps: AppsPage,
  network: NetworkPage,
  boost: BoostPage,
  ai: AiPage,
  settings: SettingsPage,
};

const isRoute = (r: string | null | undefined): r is Route => !!r && r in PAGES;

export default function App() {
  const [route, setRoute] = useState<Route>("dashboard");
  const [version, setVersion] = useState("0.1.0");
  const [elevated, setElevated] = useState<boolean | null>(null);
  const [ready, setReady] = useState(false);
  useEffect(() => {
    // Pointer halo: one listener for the whole app; `.halo` cards only read --mx/--my.
    const move = (e: PointerEvent) => {
      const el = (e.target as Element | null)?.closest?.<HTMLElement>(".halo");
      if (!el) return;
      const r = el.getBoundingClientRect();
      el.style.setProperty("--mx", `${e.clientX - r.left}px`);
      el.style.setProperty("--my", `${e.clientY - r.top}px`);
    };
    document.addEventListener("pointermove", move, { passive: true });
    return () => document.removeEventListener("pointermove", move);
  }, []);
  useEffect(() => {
    backend.appInfo().then((i) => {
      if (i) {
        setVersion(i.version);
        setElevated(i.elevated);
        if (i.start_tab) boot.startTab = i.start_tab;
        boot.autorun = i.autorun;
        if (i.scan_path) boot.scanPath = i.scan_path;
        if (isRoute(i.start_route)) setRoute(i.start_route);
      }
      setReady(true);
    });
    return onNavigate(setRoute);
  }, []);
  useEffect(() => {
    if (!inTauri) return;
    // An .exe/.lnk dropped anywhere on the window → the uninstaller finds its app.
    const off = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type !== "drop") return;
      const path = e.payload.paths[0];
      if (!path) return;
      emitDrop(path);
      setRoute("apps");
    });
    return () => {
      void off.then((f) => f());
    };
  }, []);
  const Page = PAGES[route];
  return (
    <I18nProvider>
      <TooltipProvider>
        <AppShell route={route} onNavigate={setRoute} version={version} elevated={elevated}>
          {ready ? (
            // Only the content area enters (200 ms, 4 px); the shell stays still.
            <div key={route} className="route-enter min-h-full">
              <Page />
            </div>
          ) : null}
        </AppShell>
      </TooltipProvider>
    </I18nProvider>
  );
}
