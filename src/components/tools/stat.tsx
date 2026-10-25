import { HelpHint } from "@/components/ui/primitives";
import { cn } from "@/lib/utils";

/** One reading of an instrument: a small label over a number. */
export function Stat({
  label,
  value,
  unit,
  tone,
  help,
  size = "md",
  className,
}: {
  label: string;
  value: string;
  unit?: string;
  tone?: "success" | "warning" | "danger";
  help?: string;
  size?: "md" | "lg";
  className?: string;
}) {
  return (
    <div className={cn("min-w-0", className)}>
      <p className="flex items-center gap-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
        {label}
        {help ? <HelpHint text={help} /> : null}
      </p>
      <p
        className={cn(
          "num mt-1 truncate font-display font-semibold",
          size === "lg" ? "text-5xl" : "text-xl",
          tone === "success" && "text-success",
          tone === "warning" && "text-warning",
          tone === "danger" && "text-destructive",
        )}
      >
        {value}
        {unit && value !== "—" ? (
          <span className="ml-1 text-xs font-normal text-muted-foreground">{unit}</span>
        ) : null}
      </p>
    </div>
  );
}
