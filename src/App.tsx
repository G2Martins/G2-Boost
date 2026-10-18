import { getCurrentWebview } from "@tauri-apps/api/webview";
import { X } from "lucide-react";
import { useEffect, useState } from "react";
import { AppShell, type Route } from "@/components/shell/app-shell";
import { Button, TooltipProvider } from "@/components/ui/primitives";
import { backend, inTauri } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { I18nProvider, useI18n } from "@/lib/i18n";
import { emitDrop, onNavigate, openOrphans } from "@/lib/nav";
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

function RemovedBanner({
  name,
  onOpen,
  onClose,
}: {
  name: string;
  onOpen: () => void;
  onClose: () => void;
}) {
  const { t } = useI18n();
  return (
    <div className="animate-rise sticky top-0 z-40 flex items-center gap-3 border-b border-border-strong bg-surface/95 px-6 py-2.5 backdrop-blur">
      <span className="text-[13px]">{t("apps.removed_banner", { name })}</span>
      <Button size="sm" variant="primary" className="ml-auto" onClick={onOpen}>
        {t("apps.removed_banner.action")}
      </Button>
      <button
        type="button"
        onClick={onClose}
        aria-label={t("common.cancel")}
        className="press flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/[0.06] hover:text-foreground"
      >
        <X className="size-4" />
      </button>
    </div>
  );
}

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
      } else {
        // Browser (`bun run dev`, e2e tests): `#/route/tab` picks the page — dev aid only.
        const [r, tab] = window.location.hash.replace(/^#\/?/, "").split("/");
        if (tab) boot.startTab = tab;
        if (isRoute(r)) setRoute(r);
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
  // SmartDelete: an app removed outside G2 Boost → offer its leftovers.
  const [removed, setRemoved] = useState<string | null>(null);
  useEffect(() => {
    const off = backend.on<{ id: string; name: string }>("apps:removed", (a) => setRemoved(a.name));
    return () => {
      void off.then((f) => f());
    };
  }, []);
  const Page = PAGES[route];
  return (
    <I18nProvider>
      <TooltipProvider>
        <AppShell route={route} onNavigate={setRoute} version={version} elevated={elevated}>
          {removed ? (
            <RemovedBanner
              name={removed}
              onOpen={() => {
                setRemoved(null);
                openOrphans();
                setRoute("apps");
              }}
              onClose={() => setRemoved(null)}
            />
          ) : null}
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
