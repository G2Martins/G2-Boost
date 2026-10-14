import {
  ChevronRight,
  FileWarning,
  HardDrive,
  LayoutGrid,
  Loader2,
  Play,
  Square,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { FILE_CLASSES, Treemap } from "@/components/disk/treemap";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
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
import {
  type BigFile,
  backend,
  type DriveInfo,
  inTauri,
  type NodeView,
  type ScanProgressEvent,
  type ScanSummary,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

type Crumb = { ids: number[]; name: string };

export function DiskPage() {
  const { t, locale } = useI18n();
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [selected, setSelected] = useState<string>("C:");
  const [progress, setProgress] = useState<ScanProgressEvent | null>(null);
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<NodeView | null>(null);
  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [top, setTop] = useState<BigFile[]>([]);
  const scanning = progress !== null;
  const started = useRef(false);

  const loadRoot = useCallback(async () => {
    const v = await backend.diskTree([], 2, 80);
    if (v) {
      setView(v);
      setCrumbs([{ ids: [], name: v.name }]);
    }
    setTop((await backend.diskTopFiles(50)) ?? []);
  }, []);

  const startScan = useCallback(async (root: string) => {
    setError(null);
    setSummary(null);
    setView(null);
    setTop([]);
    setProgress({ files: 0, dirs: 0, bytes: 0, errors: 0, current: root });
    try {
      await backend.diskScanStart(root.endsWith("\\") ? root : `${root}\\`);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
      setProgress(null);
    }
  }, []);

  useEffect(() => {
    const offs: Promise<() => void>[] = [
      backend.on<ScanProgressEvent>("disk:progress", (p) => setProgress(p)),
      backend.on<ScanSummary>("disk:done", async (s) => {
        setSummary(s);
        setProgress(null);
        await loadRoot();
      }),
      backend.on<string>("disk:error", (e) => {
        setError(e);
        setProgress(null);
      }),
    ];
    backend.listDrives().then((d) => {
      if (d?.length) {
        setDrives(d);
        setSelected((cur) => (d.some((x) => x.letter === cur) ? cur : (d[0]?.letter ?? "C:")));
      }
      if (boot.autorun && !started.current) {
        started.current = true;
        void startScan(boot.scanPath ?? d?.[0]?.letter ?? "C:");
      }
    });
    // a previous scan may still be in memory (page re-mount)
    backend.diskSummary().then((s) => {
      if (s) {
        setSummary(s);
        void loadRoot();
      }
    });
    return () => {
      for (const p of offs) p.then((off) => off());
    };
  }, [loadRoot, startScan]);

  const open = async (node: NodeView) => {
    const v = await backend.diskTree(node.ids, 2, 80);
    if (!v) return;
    setView(v);
    setCrumbs((c) => [...c, { ids: v.ids, name: v.name }]);
  };
  const jump = async (i: number) => {
    const c = crumbs[i];
    if (!c) return;
    const v = await backend.diskTree(c.ids, 2, 80);
    if (!v) return;
    setView(v);
    setCrumbs((cs) => cs.slice(0, i + 1));
  };

  const current = drives.find((d) => d.letter === selected);
  const initialTab = ["treemap", "largest", "essentials"].includes(boot.startTab ?? "")
    ? (boot.startTab as string)
    : "treemap";

  return (
    <PageHeader
      title={t("disk.title")}
      subtitle={t("disk.subtitle")}
      actions={
        scanning ? (
          <Button variant="outline" size="lg" onClick={() => backend.diskScanCancel()}>
            <Square className="size-4" />
            {t("common.stop")}
          </Button>
        ) : (
          <Button
            variant="primary"
            size="lg"
            onClick={() => startScan(selected)}
            disabled={!inTauri}
          >
            <Play className="size-4" />
            {t("disk.scan")}
          </Button>
        )
      }
      tabs={
        <Tabs defaultValue={initialTab}>
          <TabsList>
            <TabsTrigger value="treemap">{t("disk.tab.treemap")}</TabsTrigger>
            <TabsTrigger value="largest">{t("disk.tab.largest")}</TabsTrigger>
            <TabsTrigger value="essentials">{t("disk.tab.essentials")}</TabsTrigger>
          </TabsList>
          <TabsContent value="treemap">
            <PageBody className="px-0 pb-0">
              <DriveBar
                drives={drives}
                selected={selected}
                onSelect={setSelected}
                current={current}
                progress={progress}
                summary={summary}
              />
              {error ? (
                <Card className="border-destructive/40 text-destructive">
                  {t("disk.error", { error })}
                </Card>
              ) : null}
              <Card className="animate-rise" style={rise(1)}>
                <CardTitle
                  help={t("disk.help.treemap")}
                  right={
                    view ? (
                      <span className="flex flex-wrap items-center gap-2 text-xs">
                        {FILE_CLASSES.map((c) => (
                          <span key={c} className="flex items-center gap-1 text-muted-foreground">
                            <span
                              className="size-2.5 rounded-sm"
                              style={{ background: `hsl(var(--class-${c}))` }}
                            />
                            {t(`disk.class.${c}` as I18nKey)}
                          </span>
                        ))}
                      </span>
                    ) : null
                  }
                >
                  {t("disk.tab.treemap")}
                </CardTitle>
                {view ? (
                  <>
                    <nav
                      className="mb-3 flex flex-wrap items-center gap-1 text-xs"
                      aria-label="breadcrumb"
                    >
                      {crumbs.map((c, i) => (
                        <span key={c.ids.join("/") || "root"} className="flex items-center gap-1">
                          {i > 0 ? <ChevronRight className="size-3 text-muted-foreground" /> : null}
                          <button
                            type="button"
                            onClick={() => jump(i)}
                            className={cn(
                              "rounded px-1.5 py-0.5 font-display font-semibold",
                              i === crumbs.length - 1
                                ? "bg-card-hover text-foreground"
                                : "text-muted-foreground hover:text-foreground",
                            )}
                          >
                            {c.name}
                          </button>
                        </span>
                      ))}
                      <span className="num ml-auto text-muted-foreground">
                        {formatBytes(view.size, locale)} · {view.files.toLocaleString(locale)}{" "}
                        {t("disk.files")}
                        {view.others_size > 0
                          ? ` · +${view.others_count} ${t("disk.others")} (${formatBytes(view.others_size, locale, 0)})`
                          : ""}
                      </span>
                    </nav>
                    <Treemap root={view} onOpen={open} locale={locale} />
                    <p className="mt-2 text-[11px] text-muted-foreground">{t("disk.hint.open")}</p>
                  </>
                ) : (
                  <EmptyState
                    icon={
                      scanning ? (
                        <Loader2 className="size-5 animate-spin text-primary" />
                      ) : (
                        <LayoutGrid className="size-5" />
                      )
                    }
                    title={scanning ? t("disk.scanning") : t("disk.empty.title")}
                    body={scanning ? progress?.current : t("disk.empty.body")}
                  />
                )}
              </Card>
            </PageBody>
          </TabsContent>
          <TabsContent value="largest">
            <PageBody className="px-0 pb-0">
              <Card className="animate-rise">
                <CardTitle>{t("disk.tab.largest")}</CardTitle>
                {top.length ? (
                  <table className="w-full text-xs">
                    <thead className="text-muted-foreground">
                      <tr className="border-b border-border">
                        <th className="py-2 text-left font-medium">#</th>
                        <th className="py-2 text-left font-medium">{t("disk.col.path")}</th>
                        <th className="py-2 text-right font-medium">{t("disk.col.size")}</th>
                        <th className="py-2 text-right font-medium">{t("disk.col.modified")}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {top.map((f, i) => (
                        <tr key={f.path} className="border-b border-border/60 last:border-0">
                          <td className="num py-1.5 text-muted-foreground">{i + 1}</td>
                          <td className="num max-w-[640px] truncate py-1.5" title={f.path}>
                            {f.path}
                          </td>
                          <td className="num py-1.5 text-right font-semibold">
                            {formatBytes(f.size, locale)}
                          </td>
                          <td className="num py-1.5 text-right text-muted-foreground">
                            {f.mtime ? new Date(f.mtime * 1000).toLocaleDateString(locale) : "—"}
                          </td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                ) : (
                  <EmptyState
                    icon={<HardDrive className="size-5" />}
                    title={t("disk.empty.title")}
                    body={t("disk.empty.largest")}
                  />
                )}
              </Card>
            </PageBody>
          </TabsContent>
          <TabsContent value="essentials">
            <PageBody className="px-0 pb-0">
              <Card className="animate-rise">
                <CardTitle
                  help={t("disk.help.essentials")}
                  right={
                    <Badge className="bg-card-hover text-muted-foreground">
                      {t("common.soon")}
                    </Badge>
                  }
                >
                  {t("disk.tab.essentials")}
                </CardTitle>
                <EmptyState
                  icon={<FileWarning className="size-5" />}
                  title={t("common.soon")}
                  body={t("disk.help.essentials")}
                />
              </Card>
            </PageBody>
          </TabsContent>
        </Tabs>
      }
    />
  );
}

function DriveBar({
  drives,
  selected,
  onSelect,
  current,
  progress,
  summary,
}: {
  drives: DriveInfo[];
  selected: string;
  onSelect: (l: string) => void;
  current?: DriveInfo;
  progress: ScanProgressEvent | null;
  summary: ScanSummary | null;
}) {
  const { t, locale } = useI18n();
  const list = drives.length
    ? drives
    : [{ letter: "C:", kind: "fixed", total_bytes: 0, free_bytes: 0 } as DriveInfo];
  return (
    <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
      <span className="font-display text-[15px] font-semibold tracking-wide">
        {t("disk.drive")}
      </span>
      <div className="flex flex-wrap gap-2">
        {list.map((d) => (
          <button
            key={d.letter}
            type="button"
            onClick={() => onSelect(d.letter)}
            disabled={progress !== null}
            className={cn(
              "flex items-center gap-2 rounded-md border px-3 py-2 font-display text-[13px] font-semibold transition-colors disabled:opacity-60",
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
      <span className="ml-auto text-right text-xs text-muted-foreground">
        {progress ? (
          <span className="flex items-center gap-2">
            <Loader2 className="size-3.5 animate-spin text-primary" />
            <span className="num">
              {t("disk.progress", {
                files: progress.files.toLocaleString(locale),
                size: formatBytes(progress.bytes, locale, 1),
              })}
            </span>
            <span className="max-w-[320px] truncate">{progress.current}</span>
          </span>
        ) : summary ? (
          <span className="num">
            {t("disk.summary", {
              files: summary.files.toLocaleString(locale),
              size: formatBytes(summary.bytes, locale, 1),
              seconds: (summary.elapsed_ms / 1000).toFixed(1),
            })}
            {summary.errors ? ` · ${t("disk.skipped", { count: summary.errors })}` : ""}
          </span>
        ) : current && current.total_bytes > 0 ? (
          <>
            {t("disk.free_of", {
              free: formatBytes(current.free_bytes, locale, 0),
              total: formatBytes(current.total_bytes, locale, 0),
            })}
            <DiskBar free={current.free_bytes} total={current.total_bytes} />
          </>
        ) : null}
      </span>
    </Card>
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
