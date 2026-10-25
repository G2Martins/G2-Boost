import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import {
  Activity,
  Bot,
  BrushCleaning,
  Cpu,
  Flag,
  Gamepad2,
  Gauge,
  HardDrive,
  Layers,
  LayoutDashboard,
  LayoutGrid,
  type LucideIcon,
  MonitorCheck,
  MonitorCog,
  Mouse,
  MousePointerClick,
  PanelLeftClose,
  PanelLeftOpen,
  PictureInPicture2,
  Radar,
  Rocket,
  Settings,
  ShieldAlert,
  ShieldCheck,
  ShieldHalf,
  SquareTerminal,
  Waypoints,
  Wrench,
} from "lucide-react";
import { type ReactNode, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export type Route =
  | "dashboard"
  | "mypc"
  | "hwmonitor"
  | "maintenance"
  | "security"
  | "setup"
  | "debloat"
  | "boost"
  | "peripherals"
  | "profiles"
  | "gpu"
  | "games"
  | "apps"
  | "cleanup"
  | "disk"
  | "network"
  | "overlay"
  | "mousetest"
  | "monitortest"
  | "packetloss"
  | "console"
  | "ai"
  | "settings";

type NavItem = { route: Route; icon: LucideIcon; label: I18nKey };

export const NAV: { group: I18nKey; items: NavItem[] }[] = [
  {
    group: "nav.group.main",
    items: [
      { route: "dashboard", icon: LayoutDashboard, label: "nav.dashboard" },
      { route: "mypc", icon: Cpu, label: "nav.mypc" },
      { route: "hwmonitor", icon: Activity, label: "nav.hwmonitor" },
      { route: "maintenance", icon: Wrench, label: "maintenance.nav" },
      { route: "security", icon: ShieldAlert, label: "security.nav" },
    ],
  },
  {
    group: "nav.group.optimize",
    items: [
      { route: "setup", icon: Rocket, label: "nav.setup" },
      { route: "debloat", icon: Flag, label: "nav.debloat" },
      { route: "boost", icon: Gauge, label: "nav.boost" },
      { route: "peripherals", icon: Mouse, label: "nav.peripherals" },
      { route: "profiles", icon: Layers, label: "nav.profiles" },
      { route: "gpu", icon: MonitorCog, label: "gpu.nav" },
      { route: "games", icon: Gamepad2, label: "nav.games" },
      { route: "apps", icon: LayoutGrid, label: "nav.apps" },
      { route: "cleanup", icon: BrushCleaning, label: "nav.cleanup" },
      { route: "disk", icon: HardDrive, label: "nav.disk" },
    ],
  },
  {
    group: "nav.group.tools",
    items: [
      { route: "network", icon: Radar, label: "nav.network" },
      { route: "overlay", icon: PictureInPicture2, label: "nav.overlay" },
      { route: "mousetest", icon: MousePointerClick, label: "nav.mousetest" },
      { route: "monitortest", icon: MonitorCheck, label: "nav.monitortest" },
      { route: "packetloss", icon: Waypoints, label: "nav.packetloss" },
      { route: "console", icon: SquareTerminal, label: "nav.console" },
    ],
  },
  {
    group: "nav.group.system",
    items: [
      { route: "ai", icon: Bot, label: "nav.ai" },
      { route: "settings", icon: Settings, label: "nav.settings" },
    ],
  },
];

const COLLAPSED_KEY = "g2boost.nav.collapsed";
// Below this width the content keeps the room it had with the old icon rail.
const NARROW = "(max-width: 1239px)";

function readCollapsed(): boolean {
  try {
    return localStorage.getItem(COLLAPSED_KEY) === "1";
  } catch {
    return false;
  }
}

function useNarrow(): boolean {
  const [narrow, setNarrow] = useState(() => window.matchMedia(NARROW).matches);
  useEffect(() => {
    const mq = window.matchMedia(NARROW);
    const on = () => setNarrow(mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return narrow;
}

export function AppShell({
  route,
  onNavigate,
  version,
  elevated,
  badges,
  children,
}: {
  route: Route;
  onNavigate: (r: Route) => void;
  version: string;
  /** null until the app info arrives. */
  elevated: boolean | null;
  /** Small count on a nav item (new exposure alerts on Segurança). */
  badges?: Partial<Record<Route, number>>;
  children: ReactNode;
}) {
  const { t } = useI18n();
  const narrow = useNarrow();
  const [userCollapsed, setUserCollapsed] = useState(readCollapsed);
  const collapsed = userCollapsed || narrow;
  const toggle = () => {
    const next = !userCollapsed;
    setUserCollapsed(next);
    try {
      localStorage.setItem(COLLAPSED_KEY, next ? "1" : "0");
    } catch {
      // private mode / blocked storage: the toggle still works for this session
    }
  };
  const group = NAV.find((g) => g.items.some((i) => i.route === route));
  const current = group?.items.find((i) => i.route === route);

  // One pill glides to the active item (measured), instead of each item lighting up on its own.
  const navRef = useRef<HTMLElement>(null);
  const pillRef = useRef<HTMLSpanElement>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: re-measure when the route or the width mode changes
  useLayoutEffect(() => {
    const nav = navRef.current;
    const pill = pillRef.current;
    const active = nav?.querySelector<HTMLElement>('[aria-current="page"]');
    if (!nav || !pill) return;
    if (!active) {
      pill.style.opacity = "0";
      return;
    }
    pill.style.opacity = "1";
    pill.style.height = `${active.offsetHeight}px`;
    pill.style.translate = `0 ${active.offsetTop}px`;
    if (!pill.dataset.ready) requestAnimationFrame(() => (pill.dataset.ready = "1"));
  }, [route, collapsed]);

  return (
    <div className="carbon grain relative flex h-full">
      <aside
        className={cn(
          "flex shrink-0 flex-col border-r border-border bg-surface transition-[width] duration-300 ease-out-expo",
          collapsed ? "w-16" : "w-58",
        )}
      >
        <button
          type="button"
          onClick={() => onNavigate("dashboard")}
          aria-label={t("app.name")}
          className={cn(
            "flex h-14 shrink-0 items-center gap-2.5",
            collapsed ? "justify-center" : "px-4",
          )}
        >
          <Logo className="size-7 shrink-0" />
          {collapsed ? null : (
            <span className="font-display text-[13px] font-semibold tracking-[0.2em]">
              G2 BOOST
            </span>
          )}
        </button>
        <nav
          ref={navRef}
          aria-label="main"
          className="nav-scroll relative min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-2 pb-2"
        >
          <span ref={pillRef} aria-hidden className="nav-pill" />
          {NAV.map((g, gi) => (
            // Not positioned: the buttons' offsetTop must be relative to the nav (the pill's box).
            <div key={g.group}>
              {collapsed ? (
                gi > 0 ? (
                  <div className="mx-auto my-2 h-px w-6 bg-border" />
                ) : null
              ) : (
                <p className="px-3 pt-4 pb-1.5 text-[10px] font-semibold uppercase tracking-[0.16em] text-muted-foreground/70">
                  {t(g.group)}
                </p>
              )}
              {g.items.map((item) => (
                <NavButton
                  key={item.route}
                  active={route === item.route}
                  collapsed={collapsed}
                  icon={item.icon}
                  label={t(item.label)}
                  badge={badges?.[item.route]}
                  onClick={() => onNavigate(item.route)}
                />
              ))}
            </div>
          ))}
        </nav>
        <div
          className={cn(
            "flex shrink-0 items-center border-t border-border py-2",
            collapsed ? "flex-col gap-1" : "justify-between px-3",
          )}
        >
          <span className="num text-[10px] text-muted-foreground">
            {t("app.version", { version })}
          </span>
          {narrow ? null : (
            <button
              type="button"
              onClick={toggle}
              aria-label={collapsed ? t("nav.expand") : t("nav.collapse")}
              title={collapsed ? t("nav.expand") : t("nav.collapse")}
              className="press flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-foreground/6 hover:text-foreground"
            >
              {collapsed ? (
                <PanelLeftOpen className="size-4" />
              ) : (
                <PanelLeftClose className="size-4" />
              )}
            </button>
          )}
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="flex h-11 shrink-0 items-center gap-4 border-b border-border bg-surface/80 px-5 backdrop-blur">
          {group && current ? (
            <span className="flex items-center gap-2 text-[12px]">
              <span className="text-muted-foreground">{t(group.group)}</span>
              <span className="h-3 w-px bg-border-strong" aria-hidden />
              <span className="font-semibold">{t(current.label)}</span>
            </span>
          ) : null}
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
  collapsed,
  icon: Icon,
  label,
  badge,
  onClick,
}: {
  active: boolean;
  collapsed: boolean;
  icon: LucideIcon;
  label: string;
  badge?: number;
  onClick: () => void;
}) {
  const button = (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      aria-current={active ? "page" : undefined}
      className={cn(
        "press relative z-10 flex w-full items-center rounded-md transition-colors duration-200 ease-out-expo",
        collapsed ? "h-10 justify-center" : "h-8.5 gap-3 px-3",
        active
          ? "text-foreground"
          : "text-muted-foreground hover:bg-foreground/4 hover:text-foreground",
      )}
    >
      <Icon
        className={cn("shrink-0", collapsed ? "size-5" : "size-4.5", active && "text-primary")}
      />
      {collapsed ? null : <span className="truncate text-[13px] font-medium">{label}</span>}
      {badge ? (
        <span
          className={cn(
            "num flex h-4 min-w-4 items-center justify-center rounded-full bg-warning px-1 text-[9px] font-bold text-background",
            collapsed ? "absolute top-1 right-1.5" : "ml-auto",
          )}
        >
          {badge > 9 ? "9+" : badge}
        </span>
      ) : null}
    </button>
  );
  if (!collapsed) return button;
  return (
    <TooltipPrimitive.Root delayDuration={120}>
      <TooltipPrimitive.Trigger asChild>{button}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side="right"
          sideOffset={10}
          className="tip z-50 rounded-md border border-border-strong bg-surface px-2.5 py-1.5 text-xs font-semibold text-foreground shadow-xl"
        >
          {label}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}

const LOGO_TWO = "M 706 214 A 76 76 0 1 1 842 270 L 712 396 L 884 396";
const LOGO_HEX = "M 358 560 L 414 463 L 526 463 L 582 560 L 526 657 L 414 657 Z";

/** G2 mark "G²": heavy G with ball terminal, split-hex core, exponent 2 carved out of the G.
 *  Same geometry as src-tauri/icons/g2boost.svg, without the tile, colored by tokens. */
export function Logo({ className }: { className?: string }) {
  const carve = `g2logo${useId().replace(/[^\w-]/g, "")}`;
  return (
    <svg viewBox="80 70 870 870" className={className} aria-hidden>
      <g transform="translate(-20 26)">
        <mask id={carve} maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
          <rect width="1024" height="1024" fill="white" />
          <path
            d={LOGO_TWO}
            fill="none"
            stroke="black"
            strokeWidth="160"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path d={LOGO_HEX} fill="black" />
        </mask>
        <g mask={`url(#${carve})`} fill="hsl(var(--foreground))">
          <path
            d="M 552.5 365.1 A 225 225 0 1 0 665 560 L 520 560"
            fill="none"
            stroke="hsl(var(--foreground))"
            strokeWidth="160"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <circle cx="552.5" cy="365.1" r="96" />
        </g>
        <path d="M 382 560 L 426 484 L 514 484 L 426 636 Z" fill="hsl(var(--primary))" />
        <path d="M 514 484 L 558 560 L 514 636 L 426 636 Z" fill="hsl(var(--primary))" />
        <path
          d="M 514 484 L 558 560 L 514 636 L 426 636 Z"
          fill="hsl(var(--background))"
          opacity="0.38"
        />
        <path
          d={LOGO_TWO}
          fill="none"
          stroke="hsl(var(--primary))"
          strokeWidth="100"
          strokeLinecap="round"
          strokeLinejoin="round"
        />
      </g>
    </svg>
  );
}
