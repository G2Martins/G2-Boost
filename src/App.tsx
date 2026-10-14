import { useEffect, useState } from "react";
import { AppShell, type Route } from "@/components/shell/app-shell";
import { TooltipProvider } from "@/components/ui/primitives";
import { backend } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { I18nProvider } from "@/lib/i18n";
import { BoostPage } from "@/pages/boost";
import { CleanupPage } from "@/pages/cleanup";
import { DiskPage } from "@/pages/disk";
import { NetworkPage } from "@/pages/network";
import { SettingsPage } from "@/pages/settings";

const PAGES: Record<Route, () => React.JSX.Element> = {
  disk: DiskPage,
  cleanup: CleanupPage,
  network: NetworkPage,
  boost: BoostPage,
  settings: SettingsPage,
};

const isRoute = (r: string | null | undefined): r is Route => !!r && r in PAGES;

export default function App() {
  const [route, setRoute] = useState<Route>("disk");
  const [version, setVersion] = useState("0.1.0");
  const [ready, setReady] = useState(false);
  useEffect(() => {
    backend.appInfo().then((i) => {
      if (i) {
        setVersion(i.version);
        if (i.start_tab) boot.startTab = i.start_tab;
        boot.autorun = i.autorun;
        if (isRoute(i.start_route)) setRoute(i.start_route);
      }
      setReady(true);
    });
  }, []);
  const Page = PAGES[route];
  return (
    <I18nProvider>
      <TooltipProvider>
        <AppShell route={route} onNavigate={setRoute} version={version}>
          {ready ? (
            <div key={route} className="min-h-full">
              <Page />
            </div>
          ) : null}
        </AppShell>
      </TooltipProvider>
    </I18nProvider>
  );
}
