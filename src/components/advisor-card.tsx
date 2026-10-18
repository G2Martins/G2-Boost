import { RefreshCw, Sparkle } from "lucide-react";
import { useEffect, useState } from "react";
import { LuliMark, SEVERITY_DOT, useAskLuli } from "@/components/luli-card";
import { Badge, Button, Card, CardTitle } from "@/components/ui/primitives";
import { inTauri } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { useLuli } from "@/lib/luli";
import { cn } from "@/lib/utils";

/** Luli's local findings (rules, no AI call): upgrades, free fixes, temperatures, security, gaming tier. */
export function AdvisorCard({ style }: { style?: React.CSSProperties }) {
  const { t } = useI18n();
  const { report, loadReport } = useLuli();
  const askLuli = useAskLuli();
  const [busy, setBusy] = useState(false);
  const load = async (force: boolean) => {
    setBusy(true);
    await loadReport(force);
    setBusy(false);
  };
  // biome-ignore lint/correctness/useExhaustiveDependencies: once on mount (cached in the Luli session)
  useEffect(() => {
    void load(false);
  }, []);

  const g = report?.gaming;
  return (
    <Card className="animate-rise" style={style}>
      <CardTitle
        help={t("advisor.hint")}
        right={
          <span className="flex gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={busy || !inTauri}
              onClick={() => load(true)}
              aria-label={t("mypc.refresh")}
            >
              <RefreshCw className={cn("size-4", busy && "animate-spin")} />
            </Button>
            <Button
              variant="outline"
              size="sm"
              onClick={() => askLuli({ label: "ai.quick.analyze" })}
            >
              <Sparkle className="size-4" />
              {t("advisor.ask_ai")}
            </Button>
          </span>
        }
      >
        <LuliMark className="size-6" />
        {t("advisor.title")}
      </CardTitle>
      {g?.tier ? (
        <p className="mb-3 text-sm">
          <span className="font-display font-semibold">{t(`advisor.tier.${g.tier}` as const)}</span>
          <span className="text-muted-foreground">
            {" · "}
            {t("advisor.target", { target: g.target ?? "—" })}
            {g.likely_bottleneck
              ? ` · ${t(`advisor.bottleneck.${g.likely_bottleneck}` as const)}`
              : ""}
          </span>
        </p>
      ) : null}
      {report && report.findings.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("advisor.none")}</p>
      ) : null}
      <ul className="divide-y divide-border">
        {(report?.findings ?? []).map((f) => (
          <li key={`${f.id}-${JSON.stringify(f.params)}`} className="flex gap-3 py-2.5">
            <span
              className={cn("mt-1.5 size-2 shrink-0 rounded-full", SEVERITY_DOT[f.severity])}
              aria-hidden
            />
            <div className="min-w-0 flex-1">
              <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
                {t(`advisor.${f.id}.title` as I18nKey, f.params)}
                <Badge tone="neutral">{t(`advisor.kind.${f.kind}` as const)}</Badge>
              </p>
              <p className="mt-0.5 text-xs leading-relaxed text-muted-foreground">
                {t(`advisor.${f.id}.body` as I18nKey, f.params)}
              </p>
            </div>
          </li>
        ))}
      </ul>
    </Card>
  );
}
