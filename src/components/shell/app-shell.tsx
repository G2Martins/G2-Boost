import {
  Bell,
  CircleHelp,
  Gauge,
  HardDrive,
  type LucideIcon,
  Radar,
  Search,
  Settings,
  Sparkles,
} from "lucide-react";
import type { ReactNode } from "react";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export type Route = "disk" | "cleanup" | "network" | "boost" | "settings";

const NAV: { route: Route; icon: LucideIcon; label: I18nKey }[] = [
  { route: "disk", icon: HardDrive, label: "nav.disk" },
  { route: "cleanup", icon: Sparkles, label: "nav.cleanup" },
  { route: "network", icon: Radar, label: "nav.network" },
  { route: "boost", icon: Gauge, label: "nav.boost" },
];

export function AppShell({
  route,
  onNavigate,
  version,
  children,
}: {
  route: Route;
  onNavigate: (r: Route) => void;
  version: string;
  children: ReactNode;
}) {
  const { t } = useI18n();
  return (
    <div className="carbon relative flex h-full">
      <aside className="flex w-16 shrink-0 flex-col items-center border-r border-border bg-surface py-3">
        <button
          type="button"
          onClick={() => onNavigate("disk")}
          aria-label={t("app.name")}
          className="mb-4 flex size-10 items-center justify-center"
        >
          <Logo className="size-7" />
        </button>
        <nav className="flex flex-1 flex-col items-center gap-1" aria-label="main">
          {NAV.map((item) => (
            <NavButton
              key={item.route}
              active={route === item.route}
              icon={item.icon}
              label={t(item.label)}
              onClick={() => onNavigate(item.route)}
            />
          ))}
          <div className="my-2 h-px w-8 bg-border" />
          <NavButton
            active={route === "settings"}
            icon={Settings}
            label={t("nav.settings")}
            onClick={() => onNavigate("settings")}
          />
          <NavButton
            active={false}
            icon={CircleHelp}
            label={t("nav.help")}
            onClick={() => undefined}
          />
        </nav>
        <span className="num pb-1 text-[10px] text-muted-foreground">
          {t("app.version", { version })}
        </span>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-14 shrink-0 items-center gap-4 border-b border-border bg-surface/80 px-5 backdrop-blur">
          <div className="flex items-center gap-2.5">
            <span
              className="relative inline-flex size-2.5 rounded-full bg-success animate-pulse-dot"
              aria-hidden
            />
            <span className="font-display text-[13px] font-semibold tracking-wide">
              {t("app.on")}
            </span>
          </div>
          <div className="ml-auto flex items-center gap-3">
            <button
              type="button"
              aria-label={t("app.notifications")}
              className="flex size-9 items-center justify-center rounded-md text-muted-foreground hover:bg-card-hover hover:text-foreground"
            >
              <Bell className="size-4.5" />
            </button>
            <label className="flex h-9 w-72 items-center gap-2 rounded-md border border-border bg-background px-3 text-muted-foreground focus-within:border-muted-foreground">
              <Search className="size-4" />
              <input
                type="search"
                placeholder={t("app.search")}
                className="w-full bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
              />
            </label>
          </div>
        </header>
        <main className="relative min-h-0 flex-1 overflow-y-auto">{children}</main>
      </div>
    </div>
  );
}

function NavButton({
  active,
  icon: Icon,
  label,
  onClick,
}: {
  active: boolean;
  icon: LucideIcon;
  label: string;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      title={label}
      className={cn(
        "relative flex size-11 items-center justify-center rounded-lg transition-colors",
        active
          ? "bg-primary/15 text-primary"
          : "text-muted-foreground hover:bg-card-hover hover:text-foreground",
      )}
    >
      {active ? (
        <span className="absolute -left-2.5 h-6 w-0.5 rounded-full bg-primary" aria-hidden />
      ) : null}
      <Icon className="size-5" strokeWidth={2} />
    </button>
  );
}

/** G2 mark: two slanted bars (boost chevrons) — same geometry as src-tauri/icons/g2boost.svg. */
export function Logo({ className }: { className?: string }) {
  return (
    <svg viewBox="0 0 64 64" className={className} aria-hidden>
      <path d="M14 46 L30 12 H42 L26 46 Z" fill="hsl(var(--primary))" />
      <path d="M32 46 L48 12 H56 L40 46 Z" fill="hsl(var(--foreground))" opacity="0.9" />
    </svg>
  );
}
