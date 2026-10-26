import { getCurrentWebview } from "@tauri-apps/api/webview";
import { LucideProvider, X } from "lucide-react";
import { useEffect, useState } from "react";
import { AdminGate } from "@/components/shell/admin-gate";
import { AppShell, type Route } from "@/components/shell/app-shell";
import { Button, TooltipProvider } from "@/components/ui/primitives";
import { Toaster, toast } from "@/components/ui/toast";
import { backend, inTauri } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { I18nProvider, useI18n } from "@/lib/i18n";
import { LuliProvider } from "@/lib/luli";
import { emitDrop, navigate, onNavigate, openOrphans } from "@/lib/nav";
import { AiPage } from "@/pages/ai";
import { AppsPage } from "@/pages/apps";
import { BoostPage } from "@/pages/boost";
import { CleanupPage } from "@/pages/cleanup";
import { ConsolePage } from "@/pages/console";
import { DashboardPage } from "@/pages/dashboard";
import { DebloatPage } from "@/pages/debloat";
import { DiskPage } from "@/pages/disk";
import { GamesPage } from "@/pages/games";
import { GpuPage } from "@/pages/gpu";
import { HwMonitorPage } from "@/pages/hwmonitor";
import { MaintenancePage } from "@/pages/maintenance";
import { MonitorTestPage } from "@/pages/monitortest";
import { MouseTestPage } from "@/pages/mousetest";
import { MyPcPage } from "@/pages/mypc";
import { NetworkPage } from "@/pages/network";
import { OverlayPage } from "@/pages/overlay";
import { PacketLossPage } from "@/pages/packetloss";
import { PeripheralsPage } from "@/pages/peripherals";
import { ProfilesPage } from "@/pages/profiles";
import { SecurityPage } from "@/pages/security";
import { SettingsPage } from "@/pages/settings";
import { SetupPage } from "@/pages/setup";

const PAGES: Record<Route, () => React.JSX.Element> = {
  dashboard: DashboardPage,
  disk: DiskPage,
  cleanup: CleanupPage,
  apps: AppsPage,
  network: NetworkPage,
  games: GamesPage,
  boost: BoostPage,
  console: ConsolePage,
  ai: AiPage,
  settings: SettingsPage,
  maintenance: MaintenancePage,
  gpu: GpuPage,
  security: SecurityPage,
  mypc: MyPcPage,
  hwmonitor: HwMonitorPage,
  setup: SetupPage,
  debloat: DebloatPage,
  peripherals: PeripheralsPage,
  profiles: ProfilesPage,
  overlay: OverlayPage,
  mousetest: MouseTestPage,
  monitortest: MonitorTestPage,
  packetloss: PacketLossPage,
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
        className="press flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/6 hover:text-foreground"
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
  // "Sempre abrir como administrador" from an unelevated launch: explain, then ask UAC.
  const [askAdmin, setAskAdmin] = useState(false);
  useEffect(() => {
    // Pointer halo/glare: one listener for the whole app; `.halo` cards and `.poster` covers only
    // read --mx/--my (posters also lean toward the pointer: --rx/--ry, used under a fine pointer).
    const move = (e: PointerEvent) => {
      const el = (e.target as Element | null)?.closest?.<HTMLElement>(".halo, .poster");
      if (!el) return;
      const r = el.getBoundingClientRect();
      const x = e.clientX - r.left;
      const y = e.clientY - r.top;
      el.style.setProperty("--mx", `${x}px`);
      el.style.setProperty("--my", `${y}px`);
      if (el.classList.contains("poster")) {
        el.style.setProperty("--ry", `${((x / r.width - 0.5) * 9).toFixed(2)}deg`);
        el.style.setProperty("--rx", `${((0.5 - y / r.height) * 9).toFixed(2)}deg`);
      }
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
        setAskAdmin(i.ask_admin);
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
  // New exposure seen by the background monitor → count on the Segurança icon.
  const [exposureAlerts, setExposureAlerts] = useState(0);
  useEffect(() => {
    backend.exposureAlerts().then((a) => a && setExposureAlerts(a.length));
    const off = backend.on<unknown[]>("exposure:alerts", (a) => setExposureAlerts(a.length));
    return () => {
      void off.then((f) => f());
    };
  }, []);
  const Page = PAGES[route];
  return (
    <I18nProvider>
      <LuliProvider>
        <TooltipProvider>
          <LucideProvider strokeWidth={1.5}>
            <AppShell
              route={route}
              onNavigate={setRoute}
              version={version}
              elevated={elevated}
              badges={{ security: exposureAlerts }}
            >
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
            {askAdmin ? <AdminGate route={route} onSkip={() => setAskAdmin(false)} /> : null}
            <ToasterI18n />
          </LucideProvider>
        </TooltipProvider>
      </LuliProvider>
    </I18nProvider>
  );
}

function ToasterI18n() {
  const { t } = useI18n();
  // One look at GitHub a few seconds after launch (when on in Settings): a toast if newer.
  useEffect(() => {
    if (!inTauri) return;
    const id = window.setTimeout(async () => {
      const s = await backend.settingsGet();
      if (!s?.update_auto_check) return;
      const u = await backend.updateCheck().catch(() => null);
      if (!u?.newer) return;
      toast(t("update.toast", { version: u.latest }), {
        tone: "accent",
        duration: 12000,
        action: { label: t("update.toast.action"), run: () => navigate("settings") },
      });
    }, 4000);
    return () => window.clearTimeout(id);
  }, [t]);
  return <Toaster closeLabel={t("common.close")} />;
}
