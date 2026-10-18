import { Sparkle } from "lucide-react";
import { type CSSProperties, useEffect } from "react";
import { Card } from "@/components/ui/primitives";
import type { FindingSeverity } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { useLuli, withPageData } from "@/lib/luli";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";

/** Luli's mark: the one accent, a soft ring, a four-point spark. */
export function LuliMark({ className }: { className?: string }) {
  return (
    <span
      aria-hidden
      className={cn(
        "flex size-8 shrink-0 items-center justify-center rounded-full bg-primary/12 text-primary shadow-[inset_0_0_0_1px_hsl(var(--primary)/0.35)]",
        className,
      )}
    >
      <Sparkle className="size-4 fill-current" />
    </span>
  );
}

export const SEVERITY_DOT: Record<FindingSeverity, string> = {
  info: "bg-chart-1",
  warn: "bg-warning",
  critical: "bg-destructive",
};

/** A question this screen can ask, with the published snapshots (`usePageData` keys) it needs. */
export type LuliAsk = { label: I18nKey; data?: string[] };

/** Ask Luli from any screen: the answer streams on the Luli page while the user keeps going. */
export function useAskLuli() {
  const { t } = useI18n();
  const luli = useLuli();
  return (a: LuliAsk) => {
    const q = t(a.label);
    void luli.ask(q, { sent: a.data ? withPageData(q, a.data) : undefined });
    navigate("ai");
  };
}

/**
 * Luli on a screen: the local findings that matter here (free, no AI call) and the questions
 * worth asking about what this screen shows.
 */
export function LuliCard({
  findings = [],
  asks,
  style,
}: {
  /** Advisor finding ids relevant to this screen. */
  findings?: string[];
  asks: LuliAsk[];
  style?: CSSProperties;
}) {
  const { t } = useI18n();
  const luli = useLuli();
  const ask = useAskLuli();
  const { loadReport } = luli;
  const wantsFindings = findings.length > 0;
  useEffect(() => {
    if (wantsFindings) void loadReport();
  }, [wantsFindings, loadReport]);
  const hits = (luli.report?.findings ?? []).filter((f) => findings.includes(f.id)).slice(0, 3);
  return (
    <Card
      className="animate-rise flex flex-col gap-3 py-4 lg:flex-row lg:items-center"
      style={style}
    >
      <div className="flex min-w-0 flex-1 items-start gap-3">
        <LuliMark />
        <div className="min-w-0">
          <p className="font-display text-sm font-semibold tracking-wide">{t("luli.name")}</p>
          {hits.length > 0 ? (
            <ul className="mt-1 space-y-1">
              {hits.map((f) => (
                <li
                  key={`${f.id}-${JSON.stringify(f.params)}`}
                  className="flex items-center gap-2 text-xs"
                >
                  <span
                    className={cn("size-1.5 shrink-0 rounded-full", SEVERITY_DOT[f.severity])}
                    aria-hidden
                  />
                  <span className="truncate">
                    {t(`advisor.${f.id}.title` as I18nKey, f.params)}
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="mt-0.5 text-xs text-muted-foreground">{t("luli.card.idle")}</p>
          )}
        </div>
      </div>
      <div className="flex flex-wrap gap-1.5 lg:max-w-[55%] lg:justify-end">
        {asks.map((a) => (
          <button
            key={a.label}
            type="button"
            disabled={luli.busy}
            onClick={() => ask(a)}
            className="press rounded-full border border-border px-3 py-1.5 text-[11px] text-muted-foreground transition-colors hover:border-primary/50 hover:text-foreground disabled:opacity-40"
          >
            {t(a.label)}
          </button>
        ))}
      </div>
    </Card>
  );
}
