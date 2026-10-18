import * as SwitchPrimitive from "@radix-ui/react-switch";
import * as TabsPrimitive from "@radix-ui/react-tabs";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";
import { CircleHelp } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";
import { cn } from "@/lib/utils";

/* ---------- Button ---------- */
type ButtonVariant = "primary" | "secondary" | "ghost" | "outline";
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
        "inline-flex items-center justify-center gap-2 rounded-md font-display font-semibold tracking-wide transition-all duration-150 disabled:pointer-events-none disabled:opacity-40",
        size === "sm" && "h-8 px-3 text-xs",
        size === "md" && "h-10 px-4 text-[13px]",
        size === "lg" && "h-11 px-5 text-sm",
        variant === "primary" &&
          "bg-primary text-primary-foreground shadow-[0_0_0_1px_hsl(var(--primary)/0.4),0_6px_18px_-6px_hsl(var(--primary)/0.7)] hover:brightness-110 active:scale-[0.98]",
        variant === "secondary" && "bg-card-hover text-foreground hover:bg-border",
        variant === "outline" &&
          "border border-border bg-transparent text-foreground hover:bg-card-hover",
        variant === "ghost" &&
          "bg-transparent text-muted-foreground hover:bg-card-hover hover:text-foreground",
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
        "-mb-px rounded-t-md border-b-2 border-transparent px-4 py-2.5 font-display text-[13px] font-semibold tracking-wide text-muted-foreground transition-colors hover:text-foreground data-[state=active]:border-foreground data-[state=active]:bg-card data-[state=active]:text-foreground",
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
        "relative h-6 w-11 shrink-0 rounded-full border border-border bg-background transition-colors data-[state=checked]:border-success data-[state=checked]:bg-success/90 disabled:opacity-40",
        className,
      )}
      {...props}
    >
      <SwitchPrimitive.Thumb className="block size-5 translate-x-0.5 rounded-full bg-foreground shadow transition-transform data-[state=checked]:translate-x-[22px] data-[state=checked]:bg-background" />
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

export function Badge({
  children,
  className,
  title,
}: {
  children: ReactNode;
  className?: string;
  title?: string;
}) {
  return (
    <span
      title={title}
      className={cn(
        "rounded px-1.5 py-0.5 font-display text-[10px] font-semibold uppercase tracking-wider",
        className,
      )}
    >
      {children}
    </span>
  );
}

/* ---------- Metric row (label · value · check) ---------- */
export function MetricRow({ label, value, ok }: { label: string; value: string; ok?: boolean }) {
  return (
    <div className="flex items-center justify-between py-2.5">
      <span className="text-muted-foreground">{label}</span>
      <span className="flex items-center gap-2">
        <span className="num font-semibold">{value}</span>
        {ok === undefined ? null : (
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
    <div className="flex min-h-[280px] flex-col items-center justify-center gap-3 text-center">
      <div className="flex size-11 items-center justify-center rounded-full border border-border bg-card-hover text-muted-foreground">
        {icon}
      </div>
      <p className="font-display text-sm font-semibold">{title}</p>
      {body ? <p className="max-w-md text-xs text-muted-foreground">{body}</p> : null}
      {action}
    </div>
  );
}
