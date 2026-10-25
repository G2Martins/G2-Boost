import { ArrowUpRight, Check, ListChecks, Minus, RefreshCw, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { ElevateButton } from "@/components/elevate-button";
import type { Route } from "@/components/shell/app-shell";
import { rise } from "@/components/shell/page-header";
import { CountUp } from "@/components/ui/count-up";
import { Badge, Button, Card, CardTitle, EmptyState, Skeleton } from "@/components/ui/primitives";
import {
  backend,
  type CheckupItem,
  type CheckupReport,
  type CheckupState,
  inTauri,
} from "@/lib/backend";
import { formatMeasure, rowText, rowTitle, SECTIONS } from "@/lib/checkup";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";

const TILE: Record<CheckupState, string> = {
  ok: "bg-success/55",
  attention: "bg-warning",
  unknown: "bg-foreground/12",
};

/**
 * Meu PC › Check-up: about twenty read-only measurements, each with its verdict and a way to the
 * screen that fixes it. It only measures; nothing here changes the PC.
 */
export function CheckupTab({ onOverview }: { onOverview: () => void }) {
  const { t, locale } = useI18n();
  const [report, setReport] = useState<CheckupReport | null>(null);
  const [busy, setBusy] = useState(false);
  const started = useRef(false);

  const run = useCallback(async () => {
    setBusy(true);
    const r = await backend.checkupRun();
    if (r) setReport(r);
    setBusy(false);
  }, []);
  useEffect(() => {
    if (started.current || !inTauri) return;
    started.current = true;
    void run();
  }, [run]);

  if (!inTauri) {
    return (
      <Card className="animate-rise">
        <EmptyState icon={<ListChecks className="size-5" />} title={t("common.app_only")} />
      </Card>
    );
  }

  const byId = new Map((report?.items ?? []).map((i) => [i.id, i]));
  const { ok = 0, attention = 0, unknown = 0 } = report?.summary ?? {};
  const measured = ok + attention;
  const go = (fix: string) => (fix === "mypc" ? onOverview() : navigate(fix as Route));

  return (
    <>
      <Card className="frame-ticks animate-rise">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <div className="flex items-end gap-2">
              {report ? (
                <>
                  <CountUp
                    value={ok}
                    format={(n) => String(Math.round(n))}
                    className="num font-display text-[44px] leading-none font-semibold"
                  />
                  <span className="pb-1 text-sm text-muted-foreground">
                    {t("checkup.summary.of", { total: measured })}
                  </span>
                </>
              ) : (
                <Skeleton className="h-11 w-40" />
              )}
            </div>
            <p className="mt-2 text-xs text-muted-foreground">{t("checkup.hint")}</p>
          </div>
          <div className="flex flex-wrap items-center gap-2">
            {report && attention > 0 ? (
              <Badge tone="warning">{t("checkup.attention", { count: attention })}</Badge>
            ) : null}
            {report && unknown > 0 ? (
              <Badge tone="neutral">{t("checkup.unknown", { count: unknown })}</Badge>
            ) : null}
            {report && !busy ? (
              <span className="num text-[11px] text-muted-foreground">
                {t("checkup.took", {
                  s: new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(
                    report.took_ms / 1000,
                  ),
                })}
              </span>
            ) : null}
            <Button size="sm" variant="outline" disabled={busy} onClick={run}>
              <RefreshCw className={cn("size-4", busy && "motion-safe:animate-spin")} />
              {busy ? t("checkup.running") : t("checkup.run")}
            </Button>
          </div>
        </div>
        <div aria-hidden className="mt-4 flex gap-1">
          {SECTIONS.flatMap((s) => s.ids).map((id) => {
            const item = byId.get(id);
            return item ? (
              <span
                key={id}
                title={rowTitle(item, t, locale)}
                className={cn(
                  "h-5 flex-1 rounded-[3px] transition-colors duration-300",
                  TILE[item.state],
                )}
              />
            ) : (
              <Skeleton key={id} className="h-5 flex-1 rounded-[3px]" />
            );
          })}
        </div>
      </Card>

      {SECTIONS.map((section, si) => (
        <Card key={section.id} className="animate-rise" style={rise(si + 1)}>
          <CardTitle>{t(`checkup.section.${section.id}` as I18nKey)}</CardTitle>
          <ul className="divide-y divide-border">
            {section.ids.map((id) => {
              const item = byId.get(id);
              return item ? (
                <CheckRow key={id} item={item} elevated={report?.elevated ?? false} onFix={go} />
              ) : (
                <li key={id} className="flex items-center gap-3 py-3">
                  <Skeleton className="size-7 rounded-full" />
                  <div className="flex-1 space-y-1.5">
                    <Skeleton className="h-3.5 w-48" />
                    <Skeleton className="h-3 w-80 max-w-full" />
                  </div>
                </li>
              );
            })}
          </ul>
        </Card>
      ))}
    </>
  );
}

function StateIcon({ state }: { state: CheckupState }) {
  const { t } = useI18n();
  return (
    <span
      className={cn(
        "flex size-7 shrink-0 items-center justify-center rounded-full",
        state === "ok" && "bg-success/12 text-success",
        state === "attention" && "bg-warning/12 text-warning",
        state === "unknown" && "bg-foreground/6 text-muted-foreground",
      )}
    >
      {state === "ok" ? (
        <Check aria-hidden className="size-4" strokeWidth={2.5} />
      ) : state === "attention" ? (
        <TriangleAlert aria-hidden className="size-4" />
      ) : (
        <Minus aria-hidden className="size-4" />
      )}
      <span className="sr-only">{t(`checkup.state.${state}` as I18nKey)}</span>
    </span>
  );
}

/** Share filled 0..100 with a tick at the limit the verdict is judged against. */
function LimitBar({
  value,
  limit,
  attention,
}: {
  value: number;
  limit: number | null;
  attention: boolean;
}) {
  const pct = (n: number) => `${Math.min(100, Math.max(0, n))}%`;
  return (
    <span aria-hidden className="relative hidden h-1.5 w-24 rounded-full bg-foreground/8 sm:block">
      <span
        className={cn(
          "absolute inset-y-0 left-0 rounded-full motion-safe:transition-[width] motion-safe:duration-500",
          attention ? "bg-warning" : "bg-foreground/45",
        )}
        style={{ width: pct(value) }}
      />
      {limit !== null ? (
        <span
          className="absolute -top-0.5 -bottom-0.5 w-px bg-foreground/70"
          style={{ left: pct(limit) }}
        />
      ) : null}
    </span>
  );
}

function CheckRow({
  item,
  elevated,
  onFix,
}: {
  item: CheckupItem;
  elevated: boolean;
  onFix: (route: string) => void;
}) {
  const { t, locale } = useI18n();
  const name = rowTitle(item, t, locale);
  const measure = formatMeasure(item, t, locale);
  const fix = item.fix;
  return (
    <li className="flex flex-wrap items-center gap-x-4 gap-y-2 py-3">
      <StateIcon state={item.state} />
      <div className="min-w-0 flex-1 basis-64">
        <p className="text-[13px] font-semibold">{name}</p>
        <p className="mt-0.5 text-xs text-muted-foreground">{rowText(item, t, locale)}</p>
      </div>
      <div className="flex items-center gap-3">
        {item.unit === "pct" && item.value !== null ? (
          <LimitBar value={item.value} limit={item.limit} attention={item.state === "attention"} />
        ) : null}
        {measure !== null ? (
          <span className="num min-w-14 text-right text-[13px] font-semibold">{measure}</span>
        ) : null}
        {item.state === "unknown" && item.reason === "needs_admin" && !elevated ? (
          <ElevateButton route="mypc" tab="checkup" />
        ) : fix ? (
          item.state === "attention" ? (
            <Button size="sm" variant="outline" onClick={() => onFix(fix)}>
              {t("checkup.fix")}
              <ArrowUpRight className="size-4" />
            </Button>
          ) : (
            <Button
              size="sm"
              variant="ghost"
              aria-label={`${name}. ${t("checkup.open")}`}
              title={t("checkup.open")}
              onClick={() => onFix(fix)}
            >
              <ArrowUpRight className="size-4" />
            </Button>
          )
        ) : null}
      </div>
    </li>
  );
}
