import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/primitives";
import { cn } from "@/lib/utils";

/**
 * Dashboard KPI: icon + big number + caption + one call-to-action.
 * `tone` colours the value (score, pressure) with tokens, never literal colours.
 */
export function KpiCard({
  icon: Icon,
  label,
  value,
  caption,
  tone = "neutral",
  action,
  children,
  className,
  style,
}: {
  icon: LucideIcon;
  label: string;
  value: string;
  caption?: ReactNode;
  tone?: "neutral" | "success" | "warning" | "danger";
  action?: { label: ReactNode; onClick: () => void; disabled?: boolean; primary?: boolean };
  children?: ReactNode;
  className?: string;
  style?: React.CSSProperties;
}) {
  return (
    <section className={cn("panel panel-hover flex flex-col gap-3 p-5", className)} style={style}>
      <header className="flex items-center gap-2 font-display text-[11px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
        <span className="flex size-7 items-center justify-center rounded-md bg-primary/12 text-primary">
          <Icon className="size-4" strokeWidth={2} />
        </span>
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
        {value}
      </div>
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
