import {
  ChevronRight,
  FileWarning,
  FolderOpen,
  HardDrive,
  LayoutGrid,
  Loader2,
  Play,
  Search,
  Sparkles,
  Square,
  Trash2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DupesTab } from "@/components/disk/dupes-tab";
import { FILE_CLASSES, Treemap } from "@/components/disk/treemap";
import { useErrorText } from "@/components/elevate-button";
import { LuliCard } from "@/components/luli-card";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
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
  type DiskSearch,
  type DriveInfo,
  type Essential,
  inTauri,
  type NodeView,
  type RecycleOutcome,
  type ScanProgressEvent,
  type ScanSummary,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { sizeText, usePageData } from "@/lib/luli";
import { navigate } from "@/lib/nav";
import { cn, formatBytes } from "@/lib/utils";

type Crumb = { ids: number[]; name: string };
/** One thing the user picked to send to the Recycle Bin. */
type Pick = { ids: number[]; path: string; size: number };

export function DiskPage() {
  const { t, locale } = useI18n();
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [selected, setSelected] = useState<string>("C:");
  const [progress, setProgress] = useState<ScanProgressEvent | null>(null);
  const [summary, setSummary] = useState<ScanSummary | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<NodeView | null>(null);
  const [crumbs, setCrumbs] = useState<Crumb[]>([]);
  const [folderPick, setFolderPick] = useState<Pick[] | null>(null);
  const [folderMsg, setFolderMsg] = useState<string | null>(null);
  const [version, setVersion] = useState(0);
  const errorText = useErrorText();
  const scanning = progress !== null;
  const started = useRef(false);

  // While scanning: the root's direct children growing, as a one-level treemap.
  const live = useMemo(() => {
    const items = (progress?.top ?? []).filter((i) => i.bytes > 0);
    if (items.length === 0) return null;
    const children: NodeView[] = items.map((i, k) => ({
      name: i.name,
      size: i.bytes,
      is_dir: i.is_dir,
      files: 0,
      mtime: 0,
      ids: [k],
      children: [],
      others_size: 0,
      others_count: 0,
    }));
    const root: NodeView = {
      name: "",
      size: children.reduce((a, c) => a + c.size, 0),
      is_dir: true,
      files: 0,
      mtime: 0,
      ids: [],
      children,
      others_size: 0,
      others_count: 0,
    };
    return { root, pending: new Set(items.filter((i) => !i.done).map((i) => i.name)) };
  }, [progress]);

  const loadRoot = useCallback(async () => {
    const v = await backend.diskTree([], 2, 80);
    if (v) {
      setView(v);
      setCrumbs([{ ids: [], name: v.name }]);
    }
  }, []);

  const startScan = useCallback(async (root: string) => {
    setError(null);
    setSummary(null);
    setView(null);
    setFolderPick(null);
    setFolderMsg(null);
    setProgress({ files: 0, dirs: 0, bytes: 0, errors: 0, current: root, expected: 0, top: [] });
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
  /** After a recycle: same folder, fresh sizes; children ids changed, so re-read from the root. */
  const refreshAfterRecycle = async (o: RecycleOutcome) => {
    setFolderMsg(
      t("disk.recycle.done", { count: o.recycled, size: formatBytes(o.bytes, locale) }) +
        (o.failed ? ` ${t("disk.recycle.failed", { count: o.failed })}` : ""),
    );
    setVersion((v) => v + 1);
    setFolderPick(null);
    await loadRoot();
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
  const initialTab = ["treemap", "largest", "dupes", "essentials"].includes(boot.startTab ?? "")
    ? (boot.startTab as string)
    : "treemap";

  const luliData = useMemo(() => {
    if (!summary || !view || summary.cancelled) return null;
    const drive = drives.find((d) => summary.root.toUpperCase().startsWith(d.letter.toUpperCase()));
    const where = crumbs.map((c) => c.name).join(" > ") || summary.root;
    const rows = view.children.slice(0, 25).map((c) => {
      const date = c.mtime ? new Date(c.mtime * 1000).toISOString().slice(0, 10) : "?";
      return `- ${c.is_dir ? "folder" : "file"} ${c.name}: ${sizeText(c.size)}${c.is_dir ? `, ${c.files} files` : ""}, modified ${date}`;
    });
    return [
      `Disk scan of ${summary.root}: ${sizeText(summary.bytes)} in ${summary.files} files and ${summary.dirs} folders${drive ? `; drive free ${sizeText(drive.free_bytes)} of ${sizeText(drive.total_bytes)}` : ""}.`,
      `Folder on screen: ${where} (${sizeText(view.size)}). Largest items:`,
      ...rows,
    ].join("\n");
  }, [summary, view, crumbs, drives]);
  usePageData("disk", luliData);

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
            <TabsTrigger value="dupes">{t("disk.tab.dupes")}</TabsTrigger>
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
              <LuliCard
                style={rise(1)}
                findings={[
                  "system_disk_full",
                  "hdd_system",
                  "sata_system_with_nvme",
                  "winsat_disk_weakest",
                ]}
                asks={[
                  { label: "luli.ask.disk.safe", data: ["disk"] },
                  { label: "luli.ask.disk.why", data: ["disk"] },
                ]}
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
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() =>
                          backend.diskReveal(view.ids).catch((e) => setFolderMsg(errorText(e)))
                        }
                      >
                        <FolderOpen className="size-3.5" />
                        {t("disk.reveal")}
                      </Button>
                      {view.ids.length > 0 ? (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() =>
                            setFolderPick([
                              {
                                ids: view.ids,
                                path: crumbs.map((c) => c.name).join("\\"),
                                size: view.size,
                              },
                            ])
                          }
                        >
                          <Trash2 className="size-3.5" />
                          {t("disk.recycle")}
                        </Button>
                      ) : null}
                    </nav>
                    {folderMsg ? (
                      <p className="mb-2 text-xs text-foreground/90">{folderMsg}</p>
                    ) : null}
                    {folderPick ? (
                      <RecycleConfirm
                        picks={folderPick}
                        onCancel={() => setFolderPick(null)}
                        onDone={async (o) => {
                          setFolderPick(null);
                          await refreshAfterRecycle(o);
                        }}
                      />
                    ) : null}
                    <Treemap root={view} onOpen={open} locale={locale} />
                    <p className="mt-2 text-[11px] text-muted-foreground">{t("disk.hint.open")}</p>
                  </>
                ) : live ? (
                  <>
                    <Treemap
                      root={live.root}
                      pending={live.pending}
                      onOpen={() => undefined}
                      locale={locale}
                    />
                    <p className="mt-2 flex items-center gap-2 text-[11px] text-muted-foreground">
                      <span className="size-2.5 rounded-sm border border-primary" aria-hidden />
                      {t("disk.live.hint")}
                    </p>
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
              <FilesTab
                hasScan={summary !== null}
                version={version}
                onRecycled={refreshAfterRecycle}
              />
            </PageBody>
          </TabsContent>
          <TabsContent value="dupes">
            <PageBody className="px-0 pb-0">
              <DupesTab hasScan={summary !== null} />
            </PageBody>
          </TabsContent>
          <TabsContent value="essentials">
            <PageBody className="px-0 pb-0">
              <EssentialsTab hasScan={summary !== null} version={version} />
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
  // Bytes read over the drive's used space. Capped at 99 % until done: shadow copies and
  // protected folders are counted in "used" but cannot be read.
  const ratio =
    progress && progress.expected > 0 ? Math.min(0.99, progress.bytes / progress.expected) : null;
  // What is left to read, not a time: big files go first and the profile's millions of small
  // files come last, so a time estimate from the rate so far is wildly optimistic.
  const left = ratio !== null && progress ? Math.max(0, progress.expected - progress.bytes) : 0;
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
          <span className="flex flex-col items-end gap-1.5">
            <span className="flex items-center gap-2">
              <Loader2 className="size-3.5 animate-spin text-primary" />
              {ratio !== null ? (
                <span className="num font-display text-base font-semibold text-foreground">
                  {Math.floor(ratio * 100)}%
                </span>
              ) : null}
              <span className="num">
                {t("disk.progress", {
                  files: progress.files.toLocaleString(locale),
                  size: formatBytes(progress.bytes, locale, 1),
                })}
              </span>
              {left > 0 ? (
                <span className="num">
                  · {t("disk.left", { size: formatBytes(left, locale, 1) })}
                </span>
              ) : null}
            </span>
            <span
              className="relative block h-1.5 w-72 overflow-hidden rounded-full bg-border"
              role="progressbar"
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={ratio !== null ? Math.floor(ratio * 100) : undefined}
            >
              {ratio !== null ? (
                <span
                  className="block h-full rounded-full bg-primary transition-[width] duration-300"
                  style={{ width: `${ratio * 100}%` }}
                />
              ) : (
                <span className="absolute inset-y-0 w-1/3 animate-sweep rounded-full bg-primary/70" />
              )}
            </span>
            <span className="max-w-80 truncate text-[11px]">{progress.current}</span>
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

/** Confirmation before the Recycle Bin: what, how much, and that Windows may still ask. */
function RecycleConfirm({
  picks,
  onCancel,
  onDone,
}: {
  picks: Pick[];
  onCancel: () => void;
  onDone: (o: RecycleOutcome) => void | Promise<void>;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const total = picks.reduce((a, p) => a + p.size, 0);
  const go = async () => {
    setBusy(true);
    setErr(null);
    try {
      await onDone(await backend.diskRecycle(picks.map((p) => p.ids)));
    } catch (e) {
      setErr(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="mb-3 rounded-md border border-warning/40 bg-warning/5 p-3">
      <p className="flex items-center gap-2 text-sm font-semibold">
        <Trash2 className="size-4 text-warning" />
        {t("disk.recycle.confirm_title", { count: picks.length, size: formatBytes(total, locale) })}
      </p>
      <ul className="mt-2 space-y-0.5 text-xs">
        {picks.slice(0, 8).map((p) => (
          <li key={p.ids.join("/")} className="flex gap-2">
            <span className="num min-w-0 flex-1 truncate" title={p.path}>
              {p.path}
            </span>
            <span className="num text-muted-foreground">{formatBytes(p.size, locale)}</span>
          </li>
        ))}
        {picks.length > 8 ? <li className="text-muted-foreground">+{picks.length - 8}</li> : null}
      </ul>
      <p className="mt-2 text-[11px] text-muted-foreground">{t("disk.recycle.confirm_body")}</p>
      {err ? <p className="mt-1 text-xs text-destructive">{err}</p> : null}
      <div className="mt-3 flex justify-end gap-2">
        <Button variant="ghost" size="sm" onClick={onCancel} disabled={busy}>
          {t("common.cancel")}
        </Button>
        <Button variant="primary" size="sm" onClick={go} disabled={busy}>
          {busy ? <Loader2 className="size-3.5 animate-spin" /> : <Trash2 className="size-3.5" />}
          {t("disk.recycle.confirm")}
        </Button>
      </div>
    </div>
  );
}

const FILTER_EXAMPLES = [">1gb", "*.mp4 *.mkv", "*.iso *.zip", ">6m >500mb", "*.log"];

/** Largest files, narrowed by a filter; pick rows → Recycle Bin. */
function FilesTab({
  hasScan,
  version,
  onRecycled,
}: {
  hasScan: boolean;
  version: number;
  onRecycled: (o: RecycleOutcome) => Promise<void>;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [query, setQuery] = useState("");
  const [res, setRes] = useState<DiskSearch | null>(null);
  const [picked, setPicked] = useState<Record<string, BigFile>>({});
  const [confirm, setConfirm] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` re-runs the search after a recycle
  useEffect(() => {
    if (!hasScan) return;
    const id = setTimeout(async () => {
      setRes(await backend.diskSearch(query, 200));
      setPicked({});
    }, 250);
    return () => clearTimeout(id);
  }, [query, hasScan, version]);

  // An empty query is the plain "largest files" list (one walk, no separate top-files call).
  const files = res?.files ?? [];
  const picks = Object.values(picked);
  const keyOf = (f: BigFile) => f.ids.join("/");
  const toggle = (f: BigFile) =>
    setPicked((cur) => {
      const next = { ...cur };
      if (next[keyOf(f)]) delete next[keyOf(f)];
      else next[keyOf(f)] = f;
      return next;
    });

  if (!hasScan) {
    return (
      <Card className="animate-rise">
        <EmptyState
          icon={<HardDrive className="size-5" />}
          title={t("disk.empty.title")}
          body={t("disk.empty.largest")}
        />
      </Card>
    );
  }
  return (
    <Card className="animate-rise">
      <CardTitle help={t("disk.filter.help")}>{t("disk.tab.largest")}</CardTitle>
      <div className="flex flex-wrap items-center gap-2">
        <label className="flex min-w-65 flex-1 items-center gap-2 rounded-md border border-border bg-background px-3 py-2 focus-within:border-ring">
          <Search className="size-4 text-muted-foreground" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("disk.filter.placeholder")}
            className="num w-full bg-transparent text-sm outline-none placeholder:text-muted-foreground"
            spellCheck={false}
            maxLength={200}
          />
        </label>
        {FILTER_EXAMPLES.map((ex) => (
          <button
            key={ex}
            type="button"
            onClick={() => setQuery(ex)}
            className="num rounded-md border border-border px-2 py-1 text-[11px] text-muted-foreground hover:text-foreground"
          >
            {ex}
          </button>
        ))}
      </div>
      <div className="mt-3 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
        {res ? (
          <span className="num">
            {t("disk.filter.matched", {
              count: res.matched.toLocaleString(locale),
              size: formatBytes(res.matched_bytes, locale),
            })}
          </span>
        ) : null}
        {picks.length > 0 ? (
          <Button variant="primary" size="sm" className="ml-auto" onClick={() => setConfirm(true)}>
            <Trash2 className="size-3.5" />
            {t("disk.recycle.selected", {
              count: picks.length,
              size: formatBytes(
                picks.reduce((a, f) => a + f.size, 0),
                locale,
              ),
            })}
          </Button>
        ) : null}
      </div>
      {msg ? <p className="mt-2 text-xs text-foreground/90">{msg}</p> : null}
      {confirm ? (
        <div className="mt-3">
          <RecycleConfirm
            picks={picks.map((f) => ({ ids: f.ids, path: f.path, size: f.size }))}
            onCancel={() => setConfirm(false)}
            onDone={async (o) => {
              setConfirm(false);
              setMsg(
                t("disk.recycle.done", { count: o.recycled, size: formatBytes(o.bytes, locale) }),
              );
              await onRecycled(o);
            }}
          />
        </div>
      ) : null}
      <div className="mt-2 overflow-x-auto">
        <table className="w-full text-xs">
          <thead className="text-muted-foreground">
            <tr className="border-b border-border">
              <th className="w-8 py-2" aria-label={t("disk.select")} />
              <th className="py-2 text-left font-medium">{t("disk.col.path")}</th>
              <th className="py-2 text-right font-medium">{t("disk.col.size")}</th>
              <th className="py-2 text-right font-medium">{t("disk.col.modified")}</th>
              <th className="w-10 py-2" />
            </tr>
          </thead>
          <tbody>
            {files.map((f) => (
              <tr
                key={keyOf(f)}
                className={cn(
                  "border-b border-border/60 last:border-0",
                  picked[keyOf(f)] && "bg-primary/5",
                )}
              >
                <td className="py-1.5">
                  <input
                    type="checkbox"
                    checked={Boolean(picked[keyOf(f)])}
                    onChange={() => toggle(f)}
                    aria-label={f.path}
                    className="accent-primary"
                  />
                </td>
                <td className="num max-w-160 truncate py-1.5" title={f.path}>
                  {f.path}
                </td>
                <td className="num py-1.5 text-right font-semibold">
                  {formatBytes(f.size, locale)}
                </td>
                <td className="num py-1.5 text-right text-muted-foreground">
                  {f.mtime ? new Date(f.mtime * 1000).toLocaleDateString(locale) : "—"}
                </td>
                <td className="py-1.5 text-right">
                  <button
                    type="button"
                    title={t("disk.reveal")}
                    aria-label={t("disk.reveal")}
                    onClick={() => backend.diskReveal(f.ids).catch((e) => setMsg(errorText(e)))}
                    className="rounded p-1 text-muted-foreground hover:bg-card-hover hover:text-foreground"
                  >
                    <FolderOpen className="size-3.5" />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Card>
  );
}

/** Big items Windows' own cleanup handlers shrink (cleanup page, "System files" tab). */
const SYSTEM_FILES_FOR = ["winsxs", "windows_old"];

/** Big system items with what they are and the safe way to shrink each. */
function EssentialsTab({ hasScan, version }: { hasScan: boolean; version: number }) {
  const { t, locale } = useI18n();
  const [items, setItems] = useState<Essential[] | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `version` refreshes sizes after a recycle
  useEffect(() => {
    if (hasScan) backend.diskEssentials().then((e) => setItems(e ?? []));
  }, [hasScan, version]);
  return (
    <Card className="animate-rise">
      <CardTitle help={t("disk.help.essentials")}>{t("disk.tab.essentials")}</CardTitle>
      {!hasScan || items === null ? (
        <EmptyState
          icon={<FileWarning className="size-5" />}
          title={t("disk.empty.title")}
          body={t("disk.help.essentials")}
        />
      ) : items.length === 0 ? (
        <p className="py-6 text-center text-xs text-muted-foreground">{t("disk.ess.none")}</p>
      ) : (
        <ul className="divide-y divide-border">
          {[...items]
            .sort((a, b) => b.size - a.size)
            .map((e) => (
              <li key={e.id} className="py-3">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-display text-[14px] font-semibold">
                    {t(`disk.ess.${e.id}.title` as I18nKey)}
                  </span>
                  <span className="num text-[11px] text-muted-foreground">{e.path}</span>
                  <span className="num ml-auto text-sm font-semibold">
                    {e.size > 0 ? formatBytes(e.size, locale) : t("disk.ess.unreadable")}
                  </span>
                </div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {t(`disk.ess.${e.id}.body` as I18nKey)}
                </p>
                {SYSTEM_FILES_FOR.includes(e.id) ? (
                  <Button
                    size="sm"
                    variant="outline"
                    className="mt-2"
                    onClick={() => {
                      boot.startTab = "system";
                      navigate("cleanup");
                    }}
                  >
                    <Sparkles className="size-4" />
                    {t("volcache.from_disk")}
                  </Button>
                ) : null}
              </li>
            ))}
        </ul>
      )}
    </Card>
  );
}
