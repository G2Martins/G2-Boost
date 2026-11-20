import { History, ScanSearch, SlidersHorizontal } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { DETAIL_TABS, DetailedMode, type DetailTab } from "@/components/cleanup/detailed";
import { SmartView } from "@/components/cleanup/smart-view";
import { LuliCard } from "@/components/luli-card";
import { PageBody, PageHeader } from "@/components/shell/page-header";
import {
  Badge,
  Card,
  CardTitle,
  EmptyState,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import { backend, type RunRecord } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

type Mode = "smart" | "detailed";

/**
 * Limpeza: the smart view (Analyze → three buckets → Clean) by default; the switches per
 * category stay one click away in the detailed mode. `startTab` picks a detailed tab (`system`
 * from the disk page's essentials) or `analyze` (back from Elevate: analyze right away).
 */
export function CleanupPage() {
  const { t, locale } = useI18n();
  const [arrival] = useState(() => {
    const tab = boot.startTab;
    boot.startTab = undefined;
    return tab;
  });
  const [mode, setMode] = useState<Mode>(() =>
    DETAIL_TABS.includes(arrival as DetailTab) ? "detailed" : "smart",
  );
  const [tab, setTab] = useState<DetailTab>(() =>
    DETAIL_TABS.includes(arrival as DetailTab) ? (arrival as DetailTab) : "daily",
  );
  const [history, setHistory] = useState<RunRecord[]>([]);
  const [elevated, setElevated] = useState(false);

  useEffect(() => {
    backend.appInfo().then((i) => i && setElevated(i.elevated));
    backend.cleanupHistory().then((h) => h && setHistory(h));
  }, []);

  const detailed = (next?: string) => {
    if (next && DETAIL_TABS.includes(next as DetailTab)) setTab(next as DetailTab);
    setMode("detailed");
  };

  return (
    <>
      <PageHeader
        title={t("cleanup.title")}
        subtitle={t(mode === "smart" ? "clean.subtitle" : "cleanup.subtitle")}
        actions={
          <fieldset className="flex gap-1.5" aria-label={t("clean.mode.label")}>
            <ModeChip pressed={mode === "smart"} onClick={() => setMode("smart")}>
              <ScanSearch className="size-3.5" />
              {t("clean.mode.smart")}
            </ModeChip>
            <ModeChip pressed={mode === "detailed"} onClick={() => setMode("detailed")}>
              <SlidersHorizontal className="size-3.5" />
              {t("clean.mode.detailed")}
            </ModeChip>
          </fieldset>
        }
        tabs={
          mode === "detailed" ? (
            <Tabs value={tab} onValueChange={(v) => setTab(v as DetailTab)}>
              <TabsList>
                <TabsTrigger value="daily">{t("cleanup.tab.daily")}</TabsTrigger>
                <TabsTrigger value="monthly">{t("cleanup.tab.monthly")}</TabsTrigger>
                <TabsTrigger value="advanced">{t("cleanup.tab.advanced")}</TabsTrigger>
                <TabsTrigger value="system">{t("volcache.tab")}</TabsTrigger>
              </TabsList>
              {DETAIL_TABS.map((d) => (
                <TabsContent key={d} value={d} />
              ))}
            </Tabs>
          ) : undefined
        }
      />
      <PageBody>
        <LuliCard
          findings={["system_disk_full"]}
          asks={[
            { label: "luli.ask.cleanup.worth", data: ["cleanup"] },
            { label: "luli.ask.cleanup.often" },
          ]}
        />
        {mode === "smart" ? (
          <SmartView
            elevated={elevated}
            autostart={arrival === "analyze"}
            onHistory={setHistory}
            onDetailed={detailed}
          />
        ) : (
          <DetailedMode
            tab={tab}
            elevated={elevated}
            autorun={!!boot.autorun}
            onHistory={setHistory}
          />
        )}

        <Card className="animate-rise">
          <CardTitle>
            <History className="size-4" />
            {t("cleanup.history")}
          </CardTitle>
          {history.length === 0 ? (
            <EmptyState
              icon={<History className="size-5" />}
              title={t("cleanup.history.empty")}
              body={t("clean.history.empty")}
            />
          ) : (
            <ul className="divide-y divide-border text-xs">
              {[...history]
                .reverse()
                .slice(0, 10)
                .map((r) => (
                  <li
                    key={`${r.at}-${r.trigger}-${r.items[0]?.id ?? ""}`}
                    className="flex items-center gap-3 py-2"
                  >
                    <span className="num text-muted-foreground">
                      {new Date(r.at * 1000).toLocaleString(locale)}
                    </span>
                    <Badge tone="neutral">
                      {t(`cleanup.trigger.${triggerKey(r.trigger)}` as const)}
                    </Badge>
                    <span className="num ml-auto font-semibold">
                      {formatBytes(
                        r.items.reduce((s, i) => s + i.bytes_freed, 0),
                        locale,
                      )}
                    </span>
                    <span className="num text-muted-foreground">
                      {t("cleanup.history.secs", { secs: (r.duration_ms / 1000).toFixed(1) })}
                    </span>
                  </li>
                ))}
            </ul>
          )}
        </Card>
      </PageBody>
    </>
  );
}

/** Segmented toggle (aria-pressed), like the other pages' filters. */
function ModeChip({
  pressed,
  onClick,
  children,
}: {
  pressed: boolean;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={pressed}
      onClick={onClick}
      className={cn(
        "press flex items-center gap-1.5 rounded-md border px-3 py-1.5 text-xs font-semibold transition-colors",
        pressed
          ? "border-border-strong bg-foreground/8 text-foreground"
          : "border-border text-muted-foreground hover:text-foreground",
      )}
    >
      {children}
    </button>
  );
}

function triggerKey(trigger: string): "manual" | "routine" | "game" | "test" {
  return trigger === "routine" || trigger === "game" || trigger === "test" ? trigger : "manual";
}
