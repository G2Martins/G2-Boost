import type { CSSProperties, ReactNode } from "react";
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
    <div className="banner relative min-h-full overflow-hidden px-6 pt-7 pb-6">
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

/** Staggered reveal for lists of cards: `className="animate-rise" style={rise(i)}` (60 ms steps, CSS caps at 8). */
export function rise(index: number): CSSProperties {
  return { "--i": index } as CSSProperties;
}
