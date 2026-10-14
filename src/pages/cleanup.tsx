import { Eye, ShieldAlert, Sparkles } from "lucide-react";
import { useMemo, useState } from "react";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  HelpHint,
  Switch,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import {
  CLEANUP_CATALOG,
  type CleanupGroup,
  cleanupHint,
  cleanupName,
  type Risk,
} from "@/data/catalogs";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const RISK_KEY: Record<Risk, I18nKey> = {
  low: "cleanup.risk.low",
  medium: "cleanup.risk.medium",
  high: "cleanup.risk.high",
};
const RISK_CLASS: Record<Risk, string> = {
  low: "bg-success/12 text-success",
  medium: "bg-warning/12 text-warning",
  high: "bg-destructive/12 text-destructive",
};

export function CleanupPage() {
  const { t } = useI18n();
  const [on, setOn] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(CLEANUP_CATALOG.map((c) => [c.id, c.defaultOn])),
  );
  const [group, setGroup] = useState<CleanupGroup>("daily");
  const visible = useMemo(() => CLEANUP_CATALOG.filter((c) => c.group === group), [group]);
  const selectedCount = visible.filter((c) => on[c.id]).length;

  return (
    <>
      <PageHeader
        title={t("cleanup.title")}
        subtitle={t("cleanup.subtitle")}
        tabs={
          <Tabs value={group} onValueChange={(v) => setGroup(v as CleanupGroup)}>
            <TabsList>
              <TabsTrigger value="daily">{t("cleanup.tab.daily")}</TabsTrigger>
              <TabsTrigger value="monthly">{t("cleanup.tab.monthly")}</TabsTrigger>
              <TabsTrigger value="advanced">{t("cleanup.tab.advanced")}</TabsTrigger>
            </TabsList>
            <TabsContent value="daily" />
            <TabsContent value="monthly" />
            <TabsContent value="advanced" />
          </Tabs>
        }
      />
      <PageBody>
        <Card className="animate-rise flex items-center gap-4 py-4">
          <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
            {t("cleanup.selected_total", { count: selectedCount, size: "—" })}
            <HelpHint text={t("cleanup.help")} />
          </span>
          <div className="ml-auto flex gap-2">
            <Button variant="outline" disabled title={t("common.soon")}>
              <Eye className="size-4" />
              {t("cleanup.preview")}
            </Button>
            <Button variant="primary" disabled title={t("common.soon")}>
              <Sparkles className="size-4" />
              {t("cleanup.run")}
            </Button>
          </div>
        </Card>

        <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
          {visible.map((c, i) => (
            <Card
              key={c.id}
              className="panel-hover animate-rise flex items-start gap-4"
              style={rise(i)}
            >
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="font-display text-[14px] font-semibold tracking-wide">
                    {t(cleanupName(c.id))}
                  </h3>
                  <Badge className={RISK_CLASS[c.risk]}>{t(RISK_KEY[c.risk])}</Badge>
                  {c.admin ? (
                    <Badge
                      className="bg-card-hover text-muted-foreground"
                      title={t("common.admin_required")}
                    >
                      <ShieldAlert className="mr-1 inline size-3" />
                      {t("common.admin_required")}
                    </Badge>
                  ) : null}
                </div>
                <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
                  {t(cleanupHint(c.id))}
                </p>
                {c.requiresClosed ? (
                  <p className="mt-1.5 text-[11px] text-warning/90">
                    {t("cleanup.requires_closed", { apps: c.requiresClosed.join(", ") })}
                  </p>
                ) : null}
              </div>
              <Switch
                checked={!!on[c.id]}
                onCheckedChange={(v) => setOn((s) => ({ ...s, [c.id]: v }))}
                aria-label={t(cleanupName(c.id))}
                className={cn(
                  c.risk === "high" &&
                    "data-[state=checked]:border-destructive data-[state=checked]:bg-destructive/90",
                )}
              />
            </Card>
          ))}
        </div>
        <CardTitle className="sr-only">{t("cleanup.title")}</CardTitle>
      </PageBody>
    </>
  );
}
