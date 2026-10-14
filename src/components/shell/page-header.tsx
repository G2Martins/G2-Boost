import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** Banner + title, like the ExitLag section header. Tabs render inside it (bottom-left). */
export function PageHeader({
  title,
  subtitle,
  tabs,
  actions,
}: {
  title: string;
  subtitle?: string;
  tabs?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <div className="banner grain relative overflow-hidden border-b border-border px-6 pt-7">
      <div
        aria-hidden
        className="pointer-events-none absolute inset-y-0 right-0 w-1/2 opacity-60"
        style={{
          backgroundImage:
            "repeating-linear-gradient(115deg, hsl(var(--foreground) / 0.035) 0 1px, transparent 1px 22px)",
          maskImage: "linear-gradient(to left, black, transparent)",
        }}
      />
      <div className="relative flex items-end justify-between gap-6">
        <div className="animate-rise">
          <h1 className="font-display text-[22px] font-semibold tracking-wide">{title}</h1>
          {subtitle ? <p className="mt-1 text-xs text-muted-foreground">{subtitle}</p> : null}
        </div>
        {actions ? <div className="relative mb-1">{actions}</div> : null}
      </div>
      <div className={cn("relative mt-5", tabs ? "" : "h-2")}>{tabs}</div>
    </div>
  );
}

export function PageBody({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("flex flex-col gap-4 p-6", className)}>{children}</div>;
}

/** Staggered reveal for lists of cards: `style={rise(i)}`. */
export function rise(index: number) {
  return { animationDelay: `${Math.min(index, 8) * 55}ms` } as const;
}
