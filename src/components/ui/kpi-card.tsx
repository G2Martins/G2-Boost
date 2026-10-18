import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { CountUp } from "@/components/ui/count-up";
import { Button, Skeleton } from "@/components/ui/primitives";
import { cn } from "@/lib/utils";

/**
 * Dashboard KPI: icon + big number + caption + one call-to-action.
 * `tone` colours the value (score, pressure) with tokens, never literal colours.
 * Numeric KPIs pass `count` + `format`: the number tweens in and a skeleton holds its place
 * while `count` is null. Text KPIs keep using `value`.
 */
export function KpiCard({
  icon: Icon,
  label,
  value,
  count,
  format,
  caption,
  tone = "neutral",
  action,
  children,
  className,
  style,
}: {
  icon: LucideIcon;
  label: string;
  value?: string;
  count?: number | null;
  format?: (n: number) => string;
  caption?: ReactNode;
  tone?: "neutral" | "success" | "warning" | "danger";
  action?: { label: ReactNode; onClick: () => void; disabled?: boolean; primary?: boolean };
  children?: ReactNode;
  className?: string;
  style?: React.CSSProperties;
}) {
  return (
    <section
      className={cn("panel panel-hover halo flex flex-col gap-3 p-5", className)}
      style={style}
    >
      <header className="flex items-center gap-2 text-xs font-semibold text-muted-foreground">
        <Icon className="size-4 text-primary" strokeWidth={2} />
        {label}
      </header>
      <div
        className={cn(
          "num font-display text-[34px] font-semibold leading-none tracking-tight",
          tone === "success" && "text-success",
          tone === "warning" && "text-warning",
          tone === "danger" && "text-destructive",
        )}
      >
        {count === undefined ? (
          value
        ) : count === null ? (
          <Skeleton className="h-8.5 w-28" />
        ) : (
          <CountUp value={count} format={format ?? ((n) => String(Math.round(n)))} />
        )}
      </div>
      <div className="rule-accent" aria-hidden />
      {caption ? <p className="text-xs leading-relaxed text-muted-foreground">{caption}</p> : null}
      {children}
      {action ? (
        <Button
          variant={action.primary ? "primary" : "outline"}
          size="sm"
          className="mt-auto w-full"
          disabled={action.disabled}
          onClick={action.onClick}
        >
          {action.label}
        </Button>
      ) : null}
    </section>
  );
}
