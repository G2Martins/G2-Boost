import { useEffect, useState } from "react";
import { AppShell, type Route } from "@/components/shell/app-shell";
import { TooltipProvider } from "@/components/ui/primitives";
import { backend } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { I18nProvider } from "@/lib/i18n";
import { onNavigate } from "@/lib/nav";
import { AiPage } from "@/pages/ai";
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
