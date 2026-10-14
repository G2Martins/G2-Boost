import { useEffect, useState } from "react";
import { AppShell, type Route } from "@/components/shell/app-shell";
import { TooltipProvider } from "@/components/ui/primitives";
import { backend } from "@/lib/backend";
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

export default function App() {
  const [route, setRoute] = useState<Route>("disk");
  const [version, setVersion] = useState("0.1.0");
  useEffect(() => {
    backend.appInfo().then((i) => i && setVersion(i.version));
  }, []);
  const Page = PAGES[route];
  return (
    <I18nProvider>
      <TooltipProvider>
        <AppShell route={route} onNavigate={setRoute} version={version}>
          <div key={route} className="min-h-full">
            <Page />
          </div>
        </AppShell>
      </TooltipProvider>
    </I18nProvider>
  );
}
