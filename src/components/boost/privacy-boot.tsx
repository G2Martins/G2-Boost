import { Camera, Clock, MapPin, Mic, RefreshCw, Timer } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { ElevateButton } from "@/components/elevate-button";
import { ExeIcon } from "@/components/ui/exe-icon";
import { Badge, Button, Card, CardTitle, EmptyState, Skeleton } from "@/components/ui/primitives";
import { type AccessRecord, type BootReport, backend, inTauri } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const CAP_ICON = { webcam: Camera, microphone: Mic, location: MapPin } as const;

function when(unix: number, locale: string) {
  return unix
    ? new Date(unix * 1000).toLocaleString(locale, { dateStyle: "short", timeStyle: "short" })
    : "—";
}

/* ================= Camera, microphone, location history ================= */

export function PrivacyTab() {
  const { t, locale } = useI18n();
  const [rows, setRows] = useState<AccessRecord[] | null>(null);
  const [cap, setCap] = useState<AccessRecord["capability"]>("webcam");
  const load = useCallback(async () => setRows((await backend.privacyAccess()) ?? []), []);
  useEffect(() => {
    void load();
  }, [load]);
  // One row per app: each version of an app (Discord's app-1.0.x folders) has its own entry.
  const shown = useMemo(() => {
    const seen = new Set<string>();
    return (rows ?? [])
      .filter((r) => r.capability === cap && (r.last_start || r.last_stop))
      .filter((r) => {
        const k = r.app.toLowerCase();
        if (seen.has(k)) return false;
        seen.add(k);
        return true;
      });
  }, [rows, cap]);
  const counts = useMemo(() => {
    const c = { webcam: 0, microphone: 0, location: 0 };
    for (const r of rows ?? []) if (r.last_start || r.last_stop) c[r.capability] += 1;
    return c;
  }, [rows]);
  // Windows can leave a stale "start" after a crash: only a start in the last hour counts as now.
  const recentStart = (r: AccessRecord) => r.in_use && Date.now() / 1000 - r.last_start < 3600;
  return (
    <Card className="animate-rise">
      <CardTitle
        help={t("privacy.help")}
        right={
          <Button size="sm" variant="ghost" disabled={!inTauri} onClick={load}>
            <RefreshCw className="size-4" />
          </Button>
        }
      >
        {t("privacy.title")}
      </CardTitle>
      <div className="mb-3 flex flex-wrap gap-2">
        {(Object.keys(CAP_ICON) as AccessRecord["capability"][]).map((k) => {
          const Icon = CAP_ICON[k];
          return (
            <button
              key={k}
              type="button"
              onClick={() => setCap(k)}
              aria-pressed={cap === k}
              className={cn(
                "press flex items-center gap-2 rounded-md border px-3 py-1.5 text-xs font-semibold transition-colors",
                cap === k
                  ? "border-border-strong bg-foreground/8 text-foreground"
                  : "border-border text-muted-foreground hover:text-foreground",
              )}
            >
              <Icon className="size-3.5" />
              {t(`privacy.cap.${k}` as I18nKey)}
              <span className="num text-[10px] text-muted-foreground">{counts[k]}</span>
            </button>
          );
        })}
      </div>
      {rows === null ? (
        [0, 1, 2].map((k) => <Skeleton key={k} className="my-2 h-9 w-full" />)
      ) : shown.length === 0 ? (
        <EmptyState icon={<Camera className="size-5" />} title={t("privacy.empty")} body="" />
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-xs">
            <thead className="text-muted-foreground">
              <tr className="border-b border-border">
                <th className="py-2 text-left font-medium">{t("privacy.col.app")}</th>
                <th className="py-2 text-right font-medium">{t("privacy.col.start")}</th>
                <th className="py-2 text-right font-medium">{t("privacy.col.stop")}</th>
              </tr>
            </thead>
            <tbody>
              {shown.slice(0, 80).map((r) => (
                <tr
                  key={`${r.path}:${r.last_start}`}
                  className="border-b border-border/60 last:border-0"
                >
                  <td className="max-w-96 py-1.5">
                    <span className="flex items-center gap-2">
                      <ExeIcon path={r.icon_path} name={r.app} />
                      <span className="truncate font-semibold" title={r.path}>
                        {r.app}
                      </span>
                      {r.packaged ? <Badge tone="neutral">{t("privacy.store")}</Badge> : null}
                      {r.gone ? <Badge tone="neutral">{t("privacy.gone")}</Badge> : null}
                      {recentStart(r) ? (
                        <Badge tone="warning">{t("privacy.maybe_now")}</Badge>
                      ) : null}
                    </span>
                  </td>
                  <td className="num py-1.5 text-right">{when(r.last_start, locale)}</td>
                  <td className="num py-1.5 text-right text-muted-foreground">
                    {when(r.last_stop, locale)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="mt-3 text-[11px] text-muted-foreground">{t("privacy.note")}</p>
    </Card>
  );
}

/* ================= Boot time and what slows it ================= */

export function BootCard({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const [report, setReport] = useState<BootReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    backend
      .bootReport()
      .then(setReport)
      .catch((e) => setError(e instanceof Error ? e.message : String(e)));
  }, []);
  const last = report?.boots[0];
  const avg = report?.boots.length
    ? report.boots.reduce((a, b) => a + b.total_ms, 0) / report.boots.length
    : 0;
  return (
    <Card className="animate-rise">
      <CardTitle help={t("boot.help")}>
        <Timer className="size-4" />
        {t("boot.title")}
      </CardTitle>
      {error === "needs_admin" && !elevated ? (
        <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
          {t("boot.needs_admin")}
          <ElevateButton route="boost" />
        </div>
      ) : error ? (
        <p className="text-xs text-muted-foreground">{t("boot.unavailable")}</p>
      ) : !report ? (
        <Skeleton className="h-16 w-full" />
      ) : report.boots.length === 0 ? (
        <p className="text-xs text-muted-foreground">{t("boot.none")}</p>
      ) : (
        <>
          <div className="flex flex-wrap items-baseline gap-x-6 gap-y-1">
            <span className="num font-display text-2xl font-semibold">
              {((last?.total_ms ?? 0) / 1000).toFixed(1)} s
            </span>
            <span className="text-xs text-muted-foreground">
              {t("boot.last", { when: when(last?.at ?? 0, locale) })}
            </span>
            <span className="num text-xs text-muted-foreground">
              {t("boot.avg", { s: (avg / 1000).toFixed(1), n: report.boots.length })}
            </span>
          </div>
          {last ? (
            <p className="num mt-1 text-[11px] text-muted-foreground">
              {t("boot.split", {
                main: (last.main_ms / 1000).toFixed(1),
                post: (last.post_ms / 1000).toFixed(1),
              })}
            </p>
          ) : null}
          {report.slow.length > 0 ? (
            <>
              <p className="mt-4 mb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                {t("boot.slow")}
              </p>
              <ul className="space-y-1">
                {report.slow.slice(0, 8).map((s) => (
                  <li key={`${s.kind}:${s.name}`} className="flex items-center gap-2 text-xs">
                    <Clock className="size-3.5 text-muted-foreground" />
                    <span className="min-w-0 flex-1 truncate" title={s.name}>
                      {s.friendly || s.name}
                    </span>
                    <Badge tone="neutral">{t(`boot.kind.${s.kind}` as I18nKey)}</Badge>
                    <span className="num text-muted-foreground">
                      +{(s.avg_degradation_ms / 1000).toFixed(1)} s · {s.seen}×
                    </span>
                  </li>
                ))}
              </ul>
            </>
          ) : null}
        </>
      )}
    </Card>
  );
}
