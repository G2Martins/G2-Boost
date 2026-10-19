import { HardDrive, TriangleAlert } from "lucide-react";
import { useEffect, useState } from "react";
import { ElevateButton } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { Badge, Card, CardTitle, Skeleton } from "@/components/ui/primitives";
import { backend, type DiskHealth, type HealthStatus } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { formatBytes } from "@/lib/utils";

const TONE: Record<HealthStatus, "success" | "warning" | "danger" | "neutral"> = {
  good: "success",
  caution: "warning",
  bad: "danger",
  unknown: "neutral",
};

/** Each drive's own health report (NVMe health log / SATA S.M.A.R.T.), and what got worse. */
export function DiskHealthCard({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const [drives, setDrives] = useState<DiskHealth[] | null>(null);
  useEffect(() => {
    backend.diskHealth().then((d) => setDrives(d ?? []));
  }, []);
  const needsAdmin = !elevated && (drives ?? []).some((d) => d.needs_admin);
  return (
    <Card className="animate-rise" style={rise(6)}>
      <CardTitle
        help={t("health.help")}
        right={needsAdmin ? <ElevateButton route="dashboard" /> : undefined}
      >
        <HardDrive className="size-4" />
        {t("health.title")}
      </CardTitle>
      {drives === null ? (
        <Skeleton className="h-20 w-full" />
      ) : (
        <ul className="divide-y divide-border">
          {drives.map((d) => {
            const n = d.nvme;
            const temp =
              n?.temperature_c ??
              (d.ata.find((a) => a.id === 194)?.raw !== undefined
                ? (d.ata.find((a) => a.id === 194)?.raw ?? 0) & 0xff
                : null);
            const hours = n?.power_on_hours ?? d.ata.find((a) => a.id === 9)?.raw ?? null;
            return (
              <li key={d.key || d.index} className="py-3">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-display text-[14px] font-semibold">{d.model || "—"}</span>
                  <Badge tone="neutral">{d.bus.toUpperCase()}</Badge>
                  <Badge tone={TONE[d.status]}>{t(`health.status.${d.status}` as I18nKey)}</Badge>
                  {d.needs_admin && !elevated ? (
                    <span className="text-[11px] text-muted-foreground">
                      {t("health.needs_admin")}
                    </span>
                  ) : null}
                </div>
                {n || d.ata.length > 0 ? (
                  <p className="num mt-1 flex flex-wrap gap-x-4 gap-y-0.5 text-xs text-muted-foreground">
                    {temp !== null ? <span>{t("health.temp", { c: temp })}</span> : null}
                    {n ? <span>{t("health.wear", { pct: n.percentage_used })}</span> : null}
                    {n ? (
                      <span>
                        {t("health.spare", { pct: n.available_spare, min: n.spare_threshold })}
                      </span>
                    ) : null}
                    {n ? (
                      <span>
                        {t("health.written", { size: formatBytes(n.data_written_bytes, locale) })}
                      </span>
                    ) : null}
                    {hours !== null ? (
                      <span>{t("health.hours", { n: hours.toLocaleString(locale) })}</span>
                    ) : null}
                    {n ? <span>{t("health.unsafe", { n: n.unsafe_shutdowns })}</span> : null}
                  </p>
                ) : d.status === "unknown" && !d.needs_admin ? (
                  <p className="mt-1 text-xs text-muted-foreground">{t("health.unreadable")}</p>
                ) : null}
                {[...d.findings.filter((f) => f.severity !== "good"), ...d.changes].length > 0 ? (
                  <ul className="mt-2 space-y-1">
                    {d.changes.map((c) => (
                      <li
                        key={`c-${c.code}`}
                        className="flex items-center gap-2 text-xs text-warning"
                      >
                        <TriangleAlert className="size-3.5" />
                        {t(`health.change.${c.code}` as I18nKey, { from: c.from, to: c.to })}
                      </li>
                    ))}
                    {d.findings
                      .filter((f) => f.severity !== "good")
                      .map((f) => (
                        <li
                          key={`f-${f.code}`}
                          className={
                            f.severity === "bad"
                              ? "text-xs text-destructive"
                              : "text-xs text-warning"
                          }
                        >
                          {t(`health.finding.${f.code}` as I18nKey, { n: f.value })}
                        </li>
                      ))}
                  </ul>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
    </Card>
  );
}
