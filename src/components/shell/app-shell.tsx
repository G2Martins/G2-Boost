import {
  Bot,
  Gauge,
  HardDrive,
  LayoutDashboard,
  type LucideIcon,
  PackageX,
  Radar,
  Settings,
  ShieldCheck,
  ShieldHalf,
  Sparkles,
  SquareTerminal,
} from "lucide-react";
import type { ReactNode } from "react";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export type Route =
  | "dashboard"
  | "disk"
  | "cleanup"
  | "apps"
  | "network"
  | "boost"
  | "console"
  | "ai"
  | "settings";

const NAV: { route: Route; icon: LucideIcon; label: I18nKey }[] = [
  { route: "dashboard", icon: LayoutDashboard, label: "nav.dashboard" },
  { route: "disk", icon: HardDrive, label: "nav.disk" },
  { route: "cleanup", icon: Sparkles, label: "nav.cleanup" },
  { route: "apps", icon: PackageX, label: "nav.apps" },
  { route: "network", icon: Radar, label: "nav.network" },
  { route: "boost", icon: Gauge, label: "nav.boost" },
  { route: "console", icon: SquareTerminal, label: "nav.console" },
  { route: "ai", icon: Bot, label: "nav.ai" },
];

export function AppShell({
  route,
  onNavigate,
  version,
  elevated,
  children,
}: {
  route: Route;
  onNavigate: (r: Route) => void;
  version: string;
  /** null until the app info arrives. */
  elevated: boolean | null;
  children: ReactNode;
}) {
  const { t } = useI18n();
  return (
    <div className="carbon grain relative flex h-full">
      <aside className="flex w-16 shrink-0 flex-col items-center border-r border-border bg-surface py-3">
        <button
          type="button"
          onClick={() => onNavigate("dashboard")}
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
        </nav>
        <span className="num pb-1 text-[10px] text-muted-foreground">
          {t("app.version", { version })}
        </span>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-11 shrink-0 items-center gap-4 border-b border-border bg-surface/80 px-5 backdrop-blur">
          <span className="font-display text-[12px] font-semibold tracking-[0.18em] text-muted-foreground">
            G2 BOOST
          </span>
          {elevated === null ? null : (
            <span
              title={elevated ? undefined : t("app.standard.hint")}
              className={cn(
                "ml-auto inline-flex items-center gap-1.5 rounded-md px-2 py-1 text-[11px] font-semibold ring-1 ring-inset",
                elevated
                  ? "bg-success/10 text-success ring-success/20"
                  : "bg-foreground/5 text-muted-foreground ring-foreground/10",
              )}
            >
              {elevated ? (
                <ShieldCheck className="size-3.5" />
              ) : (
                <ShieldHalf className="size-3.5" />
              )}
              {elevated ? t("app.elevated") : t("app.standard")}
            </span>
          )}
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
        "press relative flex size-11 items-center justify-center rounded-lg transition-colors duration-200 ease-out-expo",
        active
          ? "bg-foreground/8 text-foreground"
          : "text-muted-foreground hover:bg-foreground/5 hover:text-foreground",
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
