import * as SwitchPrimitive from "@radix-ui/react-switch";
import * as TabsPrimitive from "@radix-ui/react-tabs";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { CircleHelp, X } from "lucide-react";
import {
  type ComponentProps,
  createContext,
  type ReactNode,
  useContext,
  useEffect,
  useId,
  useRef,
} from "react";
import { reducedMotion } from "@/components/ui/count-up";
import { cn } from "@/lib/utils";

/* ---------- Button ---------- */
type ButtonVariant = "primary" | "secondary" | "ghost" | "outline" | "danger";
export function Button({
  variant = "secondary",
  size = "md",
  className,
  ...props
}: ComponentProps<"button"> & { variant?: ButtonVariant; size?: "sm" | "md" | "lg" }) {
  return (
    <button
      type="button"
      className={cn(
        "press inline-flex items-center justify-center gap-2 rounded-md font-semibold transition-[background-color,border-color,color,filter,translate] duration-200 ease-out-expo disabled:pointer-events-none disabled:opacity-40",
        size === "sm" && "h-8 px-3 text-xs",
        size === "md" && "h-10 px-4 text-[13px]",
        size === "lg" && "h-11 px-5 text-sm",
        variant === "primary" &&
          "bg-primary text-primary-foreground shadow-[inset_0_1px_0_hsl(0_0%_100%/0.18),0_0_0_1px_hsl(var(--primary)/0.5)] hover:brightness-110",
        variant === "secondary" && "bg-foreground/7 text-foreground hover:bg-foreground/11",
        variant === "danger" &&
          "bg-destructive text-primary-foreground shadow-[inset_0_1px_0_hsl(0_0%_100%/0.16)] hover:brightness-110",
        variant === "outline" &&
          "border border-border-strong bg-transparent text-foreground hover:bg-foreground/5",
        variant === "ghost" &&
          "bg-transparent text-muted-foreground hover:bg-foreground/6 hover:text-foreground",
        className,
      )}
      {...props}
    />
  );
}

/** Native `<select>` styled like the inputs (keyboard + screen readers for free). */
export const selectClass =
  "h-9 rounded-md border border-border bg-background px-3 text-sm outline-none focus:border-ring disabled:opacity-50";

/* ---------- Card ---------- */
export function Card({ className, ...props }: ComponentProps<"section">) {
  return <section className={cn("panel p-5", className)} {...props} />;
}

export function CardTitle({
  children,
  help,
  right,
  className,
}: {
  children: ReactNode;
  help?: string;
  right?: ReactNode;
  className?: string;
}) {
  return (
    <header className={cn("mb-4 flex items-center justify-between gap-3", className)}>
      <h3 className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
        {children}
        {help ? <HelpHint text={help} /> : null}
      </h3>
      {right}
    </header>
  );
}

/* ---------- HelpHint (the ExitLag "?" bubble) ---------- */
export function HelpHint({ text }: { text: string }) {
  return (
    <TooltipPrimitive.Root delayDuration={150}>
      <TooltipPrimitive.Trigger asChild>
        <button
          type="button"
          aria-label={text}
          className="inline-flex size-4 items-center justify-center rounded-full text-muted-foreground/70 transition-colors hover:text-foreground"
        >
          <CircleHelp className="size-4" strokeWidth={2} />
        </button>
      </TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          sideOffset={6}
          className="z-50 max-w-72 rounded-md border border-border bg-surface px-3 py-2 text-xs leading-relaxed text-foreground shadow-xl"
        >
          {text}
          <TooltipPrimitive.Arrow className="fill-surface" />
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}

export const TooltipProvider = TooltipPrimitive.Provider;

/* ---------- Tabs (underline style) ---------- */
export const Tabs = TabsPrimitive.Root;
export const TabsContent = TabsPrimitive.Content;

export function TabsList({ className, ...props }: ComponentProps<typeof TabsPrimitive.List>) {
  return (
    <TabsPrimitive.List
      className={cn("flex items-end gap-1 border-b border-border", className)}
      {...props}
    />
  );
}

export function TabsTrigger({ className, ...props }: ComponentProps<typeof TabsPrimitive.Trigger>) {
  return (
    <TabsPrimitive.Trigger
      className={cn(
        "press -mb-px rounded-t-md border-b-2 border-transparent px-4 py-2.5 text-[13px] font-semibold text-muted-foreground transition-colors duration-200 ease-out-expo hover:text-foreground data-[state=active]:border-primary data-[state=active]:bg-black/30 data-[state=active]:text-foreground",
        className,
      )}
      {...props}
    />
  );
}

/* ---------- Switch ---------- */
export function Switch({ className, ...props }: ComponentProps<typeof SwitchPrimitive.Root>) {
  return (
    <SwitchPrimitive.Root
      className={cn(
        "relative h-6 w-11 shrink-0 rounded-full border border-border-strong bg-background transition-colors duration-200 ease-out-expo data-[state=checked]:border-foreground/80 data-[state=checked]:bg-foreground/85 disabled:opacity-40",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb className="block size-5 translate-x-0.5 rounded-full bg-foreground shadow transition-transform duration-200 ease-out-expo data-[state=checked]:translate-x-5.5 data-[state=checked]:bg-background" />
    </SwitchPrimitive.Root>
  );
}

/* ---------- Status pill / badge ---------- */
export function StatusPill({
  tone,
  children,
  className,
}: {
  tone: "success" | "warning" | "danger" | "neutral";
  children: ReactNode;
  className?: string;
}) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-2 rounded-md px-3 py-1.5 font-display text-[13px] font-semibold tracking-wide",
        tone === "success" && "bg-success/12 text-success",
        tone === "warning" && "bg-warning/12 text-warning",
        tone === "danger" && "bg-destructive/12 text-destructive",
        tone === "neutral" && "bg-card-hover text-muted-foreground",
        className,
      )}
    >
      {children}
    </span>
  );
}

/** Tinted chip: 12% fill + text + 1px inset ring of the same tone (status used sparingly). */
const BADGE_TONE = {
  neutral: "bg-foreground/6 text-muted-foreground ring-foreground/10",
  accent: "bg-primary/12 text-primary ring-primary/25",
  success: "bg-success/12 text-success ring-success/25",
  warning: "bg-warning/12 text-warning ring-warning/25",
  danger: "bg-destructive/12 text-destructive ring-destructive/25",
} as const;

export function Badge({
  children,
  className,
  title,
  tone,
}: {
  children: ReactNode;
  className?: string;
  title?: string;
  tone?: keyof typeof BADGE_TONE;
}) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex items-center rounded px-1.5 py-0.5 text-[10px] font-semibold uppercase tracking-[0.08em]",
        tone && ["ring-1 ring-inset", BADGE_TONE[tone]],
        className,
      )}
    >
      {children}
    </span>
  );
}

/* ---------- Skeleton (shimmer; static under reduced motion) ---------- */
export function Skeleton({ className }: { className?: string }) {
  return <span aria-hidden className={cn("skeleton block h-4 w-full", className)} />;
}

/* ---------- Modal (native <dialog>: focus trap, Esc and inert page for free) ---------- */
export function Modal({
  open,
  onClose,
  title,
  children,
  footer,
  closeLabel,
}: {
  open: boolean;
  onClose: () => void;
  title: ReactNode;
  children: ReactNode;
  footer?: ReactNode;
  closeLabel: string;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) {
      d.removeAttribute("data-closing");
      d.showModal();
      return;
    }
    if (!open && d.open) {
      // Exit is shorter than entry (200 vs 300 ms); close for real when it ends.
      const done = () => {
        d.removeAttribute("data-closing");
        d.close();
      };
      if (reducedMotion()) return done();
      d.setAttribute("data-closing", "");
      const timer = window.setTimeout(done, 240);
      return () => window.clearTimeout(timer);
    }
  }, [open]);
  return (
    // biome-ignore lint/a11y/useKeyWithClickEvents: the click only catches the backdrop; the keyboard closes with Esc (onCancel) or the X button
    <dialog
      ref={ref}
      className="modal"
      aria-labelledby={titleId}
      onCancel={(e) => {
        e.preventDefault(); // Esc: let the owner animate the close
        onClose();
      }}
      onClick={(e) => {
        if (e.target === ref.current) onClose(); // click on the backdrop
      }}
    >
      <div className="flex flex-col gap-4 p-5">
        <header className="flex items-start justify-between gap-4">
          <h2 id={titleId} className="font-display text-[16px] font-semibold tracking-wide">
            {title}
          </h2>
          <button
            type="button"
            onClick={onClose}
            aria-label={closeLabel}
            className="press -m-1 flex size-8 items-center justify-center rounded-md text-muted-foreground hover:bg-foreground/6 hover:text-foreground"
          >
            <X className="size-4" />
          </button>
        </header>
        <div className="text-[13px] leading-relaxed">{children}</div>
        {footer ? <footer className="flex justify-end gap-2">{footer}</footer> : null}
      </div>
    </dialog>
  );
}

/* ---------- Loading scope: every MetricRow inside shows a skeleton while loading ---------- */
const Loading = createContext(false);
export function LoadingScope({ loading, children }: { loading: boolean; children: ReactNode }) {
  return <Loading.Provider value={loading}>{children}</Loading.Provider>;
}

/* ---------- Metric row (label · value · check) ---------- */
export function MetricRow({ label, value, ok }: { label: string; value: string; ok?: boolean }) {
  const loading = useContext(Loading);
  return (
    <div className="flex items-center justify-between py-2.5">
      <span className="text-muted-foreground">{label}</span>
      <span className="flex items-center gap-2">
        {loading ? (
          <Skeleton className="h-3.5 w-24" />
        ) : (
          <span className="num font-semibold">{value}</span>
        )}
        {ok === undefined || loading ? null : (
          <span
            aria-hidden
            className={cn(
              "inline-flex size-4 items-center justify-center rounded-full text-[10px] font-bold",
              ok ? "bg-success text-background" : "bg-destructive text-background",
            )}
          >
            {ok ? "✓" : "!"}
          </span>
        )}
      </span>
    </div>
  );
}

/* ---------- Empty state ---------- */
export function EmptyState({
  icon,
  title,
  body,
  action,
}: {
  icon: ReactNode;
  title: string;
  body?: string;
  action?: ReactNode;
}) {
  return (
    <div className="flex min-h-70 flex-col items-center justify-center gap-3 text-center">
      <div className="flex size-11 items-center justify-center rounded-full border border-border bg-card-hover text-muted-foreground">
        {icon}
      </div>
      <p className="font-display text-sm font-semibold">{title}</p>
      {body ? <p className="max-w-md text-xs text-muted-foreground">{body}</p> : null}
      {action}
    </div>
  );
}
