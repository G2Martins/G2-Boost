import { HardDrive, LayoutGrid, Play } from "lucide-react";
import { useEffect, useState } from "react";
import { PageBody, PageHeader } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import { backend, type DriveInfo } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

export function DiskPage() {
  const { t, locale } = useI18n();
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [selected, setSelected] = useState<string>("C:");

  useEffect(() => {
    backend.listDrives().then((d) => {
      if (d?.length) {
        setDrives(d);
        setSelected((cur) => (d.some((x) => x.letter === cur) ? cur : (d[0]?.letter ?? "C:")));
      }
    });
  }, []);

  const current = drives.find((d) => d.letter === selected);

  return (
    <>
      <PageHeader
        title={t("disk.title")}
        subtitle={t("disk.subtitle")}
        actions={
          <Button variant="primary" size="lg" disabled title={t("common.soon")}>
            <Play className="size-4" />
            {t("disk.scan")}
          </Button>
        }
        tabs={
          <Tabs defaultValue="treemap">
            <TabsList>
              <TabsTrigger value="treemap">{t("disk.tab.treemap")}</TabsTrigger>
              <TabsTrigger value="largest">{t("disk.tab.largest")}</TabsTrigger>
              <TabsTrigger value="essentials">{t("disk.tab.essentials")}</TabsTrigger>
            </TabsList>
            <TabsContent value="treemap" />
            <TabsContent value="largest" />
            <TabsContent value="essentials" />
          </Tabs>
        }
      />
      <PageBody>
        <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
          <span className="font-display text-[15px] font-semibold tracking-wide">
            {t("disk.drive")}
          </span>
          <div className="flex flex-wrap gap-2">
            {(drives.length
              ? drives
              : [{ letter: "C:", kind: "fixed", total_bytes: 0, free_bytes: 0 } as DriveInfo]
            ).map((d) => (
              <button
                key={d.letter}
                type="button"
                onClick={() => setSelected(d.letter)}
                className={cn(
                  "flex items-center gap-2 rounded-md border px-3 py-2 font-display text-[13px] font-semibold transition-colors",
                  selected === d.letter
                    ? "border-ring bg-card-hover text-foreground"
                    : "border-border bg-background text-muted-foreground hover:text-foreground",
                )}
              >
                <HardDrive className="size-4" />
                {d.letter}
                {d.total_bytes > 0 ? (
                  <span className="num text-[11px] font-medium text-muted-foreground">
                    {formatBytes(d.free_bytes, locale, 0)}
                  </span>
                ) : null}
              </button>
            ))}
          </div>
          {current && current.total_bytes > 0 ? (
            <span className="ml-auto text-xs text-muted-foreground">
              {t("disk.free_of", {
                free: formatBytes(current.free_bytes, locale, 0),
                total: formatBytes(current.total_bytes, locale, 0),
              })}
              <DiskBar free={current.free_bytes} total={current.total_bytes} />
            </span>
          ) : null}
        </Card>

        <Card className="animate-rise" style={{ animationDelay: "60ms" }}>
          <CardTitle
            help={t("disk.help.treemap")}
            right={
              <Badge className="bg-card-hover text-muted-foreground">{t("common.soon")}</Badge>
            }
          >
            {t("disk.tab.treemap")}
          </CardTitle>
          <EmptyState
            icon={<LayoutGrid className="size-5" />}
            title={t("disk.empty.title")}
            body={t("disk.empty.body")}
          />
        </Card>
      </PageBody>
    </>
  );
}

function DiskBar({ free, total }: { free: number; total: number }) {
  const used = total > 0 ? 1 - free / total : 0;
  return (
    <span className="mt-1.5 block h-1.5 w-44 overflow-hidden rounded-full bg-border">
      <span
        className={cn(
          "block h-full rounded-full",
          used > 0.9 ? "bg-destructive" : used > 0.75 ? "bg-warning" : "bg-chart-1",
        )}
        style={{ width: `${Math.round(used * 100)}%` }}
      />
    </span>
  );
}
