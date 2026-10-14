import { useEffect, useState } from "react";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Card, CardTitle, MetricRow } from "@/components/ui/primitives";
import { type AppInfo, backend } from "@/lib/backend";
import { type Locale, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export function SettingsPage() {
  const { t, locale, setLocale } = useI18n();
  const [info, setInfo] = useState<AppInfo | null>(null);
  useEffect(() => {
    backend.appInfo().then(setInfo);
  }, []);

  return (
    <>
      <PageHeader title={t("settings.title")} subtitle={t("settings.subtitle")} />
      <PageBody>
        <Card className="animate-rise">
          <CardTitle>{t("settings.language")}</CardTitle>
          <div className="flex gap-2">
            {(["pt-BR", "en"] as Locale[]).map((l) => (
              <button
                key={l}
                type="button"
                onClick={() => setLocale(l)}
                className={cn(
                  "rounded-md border px-4 py-2.5 font-display text-[13px] font-semibold",
                  locale === l
                    ? "border-ring bg-card-hover"
                    : "border-border bg-background text-muted-foreground hover:text-foreground",
                )}
              >
                {t(`settings.language.${l}` as const)}
              </button>
            ))}
          </div>
        </Card>
        <Card className="animate-rise" style={rise(1)}>
          <CardTitle>{t("settings.about")}</CardTitle>
          <div className="divide-y divide-border">
            <MetricRow label={t("settings.version")} value={info?.version ?? "—"} />
            <MetricRow label={t("settings.core")} value={info?.core_version ?? "—"} />
            <MetricRow label={t("settings.os")} value={info?.os ?? "—"} />
            <MetricRow
              label={t("settings.elevated")}
              value={info ? (info.elevated ? t("settings.yes") : t("settings.no")) : "—"}
              ok={info?.elevated}
            />
          </div>
          <p className="mt-4 text-xs text-muted-foreground">{t("settings.privacy")}</p>
        </Card>
      </PageBody>
    </>
  );
}
