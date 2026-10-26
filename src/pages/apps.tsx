import {
  Cog,
  Database,
  File as FileIcon,
  Folder,
  HardDrive,
  Link2,
  Lock,
  PackageX,
  RefreshCw,
  RotateCcw,
  Search,
  Shield,
  ShieldOff,
  Trash2,
  Upload,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { InstallTab } from "@/components/apps/install-tab";
import { UpdatesTab } from "@/components/apps/updates-tab";
import { useErrorText } from "@/components/elevate-button";
import { LuliCard } from "@/components/luli-card";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  HelpHint,
  Modal,
  Skeleton,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import {
  type AppEntry,
  backend,
  inTauri,
  type Leftover,
  type LeftoverOutcome,
  type OrphanApp,
  type UndoRecord,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { sizeText, usePageData } from "@/lib/luli";
import { APPS_REMOVED, onDrop, onOpenOrphans, takeDrop, takeOpenOrphans } from "@/lib/nav";
import { cn, formatBytes } from "@/lib/utils";

const KIND_ICON = {
  folder: Folder,
  file: FileIcon,
  shortcut: Link2,
  registry: Database,
  service: Cog,
} as const;

const CONF_TONE = { high: "success", medium: "warning", low: "neutral" } as const;
/** The core's placeholder id for the folder-first sweep (`apps::SWEEP_ID`). */
const SWEEP_ID = "sweep:unowned";

export function AppsPage() {
  const { t } = useI18n();
  const [autoScan, setAutoScan] = useState(() => takeOpenOrphans());
  const [drop, setDrop] = useState<string | null>(() => takeDrop());
  // An installed app handed over by the Install tab: the Installed tab selects it.
  const [focus, setFocus] = useState<string | null>(null);
  // Tab on arrival: the SmartDelete banner opens the leftovers sweep and a dropped .exe the
  // Installed tab; otherwise the first tab. Dev aid: G2BOOST_START_TAB=install, installed,
  // orphans (or `unowned`, which also starts the every-drive sweep) or updates.
  const [tab, setTab] = useState(() =>
    autoScan || boot.startTab === "orphans" || boot.startTab === "unowned"
      ? "orphans"
      : drop || boot.startTab === "installed"
        ? "installed"
        : boot.startTab === "updates"
          ? "updates"
          : "install",
  );
  useEffect(
    () =>
      onDrop((p) => {
        setTab("installed");
        setDrop(p);
      }),
    [],
  );
  useEffect(
    () =>
      onOpenOrphans(() => {
        setTab("orphans");
        setAutoScan(true);
      }),
    [],
  );
  return (
    <PageHeader
      title={t("apps.title")}
      subtitle={t("apps.subtitle")}
      actions={<UndoButton />}
      tabs={
        <Tabs value={tab} onValueChange={setTab}>
          <TabsList>
            <TabsTrigger value="install">{t("install.tab")}</TabsTrigger>
            <TabsTrigger value="installed">{t("apps.tab.installed")}</TabsTrigger>
            <TabsTrigger value="orphans">{t("apps.tab.orphans")}</TabsTrigger>
            <TabsTrigger value="updates">{t("updates.tab")}</TabsTrigger>
          </TabsList>
          <TabsContent value="install">
            <PageBody className="px-0 pb-0">
              <InstallTab
                onUninstall={(id) => {
                  setFocus(id);
                  setTab("installed");
                }}
                onOpenUpdates={() => setTab("updates")}
              />
            </PageBody>
          </TabsContent>
          <TabsContent value="installed">
            <PageBody className="px-0 pb-0">
              <LuliCard
                asks={[
                  { label: "luli.ask.apps.remove", data: ["apps"] },
                  { label: "luli.ask.apps.heavy", data: ["apps"] },
                ]}
              />
              <InstalledTab
                drop={drop}
                onDropHandled={() => setDrop(null)}
                focus={focus}
                onFocusHandled={() => setFocus(null)}
              />
            </PageBody>
          </TabsContent>
          <TabsContent value="orphans">
            <PageBody className="px-0 pb-0">
              <OrphansTab autoScan={autoScan} onAutoScanned={() => setAutoScan(false)} />
            </PageBody>
          </TabsContent>
          <TabsContent value="updates">
            <PageBody className="px-0 pb-0">
              <UpdatesTab />
            </PageBody>
          </TabsContent>
        </Tabs>
      }
    />
  );
}

/** The app's own icon (extracted natively, PNG); its initial on a tinted tile until it loads. */
function AppIcon({ app, size = "size-8" }: { app: AppEntry; size?: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    backend.appsIcon(app.id).then((s) => alive && setSrc(s));
    return () => {
      alive = false;
    };
  }, [app.id]);
  return src ? (
    <img
      src={src}
      alt=""
      aria-hidden
      className={cn(size, "shrink-0 object-contain")}
      draggable={false}
    />
  ) : (
    <span
      aria-hidden
      className={cn(
        size,
        "flex shrink-0 items-center justify-center rounded-md bg-foreground/6 font-display text-[13px] font-semibold text-muted-foreground",
      )}
    >
      {app.name.slice(0, 1).toUpperCase()}
    </span>
  );
}

function fmtDate(d: string | null, locale: string) {
  if (!d || !/^\d{8}$/.test(d)) return null;
  const date = new Date(Number(d.slice(0, 4)), Number(d.slice(4, 6)) - 1, Number(d.slice(6, 8)));
  return date.toLocaleDateString(locale === "en" ? "en-US" : "pt-BR");
}

function InstalledTab({
  drop,
  onDropHandled,
  focus,
  onFocusHandled,
}: {
  drop: string | null;
  onDropHandled: () => void;
  focus: string | null;
  onFocusHandled: () => void;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [apps, setApps] = useState<AppEntry[] | null>(null);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<"name" | "size" | "date" | "use">("name");
  // Last use per app (Explorer launch log): a hint for "apps you no longer open".
  const [usage, setUsage] = useState<Record<string, number | null>>({});
  const [selected, setSelected] = useState<string | null>(null);
  const [waiting, setWaiting] = useState<string | null>(null);
  // Apps whose uninstaller finished (entry gone): their leftovers may now be removed.
  const [gone, setGone] = useState<Set<string>>(new Set());
  const [reload, setReload] = useState(0);
  const [msg, setMsg] = useState<string | null>(null);
  const [ask, setAsk] = useState<AppEntry | null>(null);

  const load = useCallback(async () => {
    const l = await backend.appsList();
    if (l) setApps(l);
    const u = await backend.appsUsage();
    if (u) setUsage(Object.fromEntries(u.map((x) => [x.id, x.last_used])));
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  // A dropped .exe/.lnk: resolved once the list (server-side) is loaded.
  useEffect(() => {
    if (!drop || !apps) return;
    backend.appsResolve(drop).then((id) => {
      if (id) {
        setSelected(id);
        setMsg(null);
      } else setMsg(t("apps.drop.unknown"));
      onDropHandled();
    });
  }, [drop, apps, onDropHandled, t]);

  // An app picked in the Install tab: select it once the list is loaded.
  useEffect(() => {
    if (!focus || !apps) return;
    if (apps.some((a) => a.id === focus)) {
      setSelected(focus);
      setMsg(null);
    } else setMsg(t("apps.error.stale"));
    onFocusHandled();
  }, [focus, apps, onFocusHandled, t]);

  // After starting an uninstaller: poll until its Uninstall entry is gone, then re-check leftovers.
  useEffect(() => {
    if (!waiting) return;
    const id = window.setInterval(async () => {
      if (!(await backend.appsInstalled(waiting))) {
        setGone((g) => new Set(g).add(waiting));
        setWaiting(null);
        setMsg(t("apps.uninstalled"));
        setReload((r) => r + 1);
      }
    }, 2000);
    return () => window.clearInterval(id);
  }, [waiting, t]);

  const rows = useMemo(() => {
    const q = query.trim().toLowerCase();
    const l = (apps ?? []).filter(
      (a) => !q || a.name.toLowerCase().includes(q) || a.publisher.toLowerCase().includes(q),
    );
    if (sort === "name")
      l.sort((a, b) => a.name.localeCompare(b.name, locale === "en" ? "en" : "pt-BR"));
    if (sort === "size") l.sort((a, b) => (b.size_bytes ?? 0) - (a.size_bytes ?? 0));
    if (sort === "date")
      l.sort((a, b) => (b.install_date ?? "").localeCompare(a.install_date ?? ""));
    // Least recently opened first; apps never seen in the launch log after them.
    if (sort === "use")
      l.sort(
        (a, b) =>
          (usage[a.id] ?? Number.MAX_SAFE_INTEGER) - (usage[b.id] ?? Number.MAX_SAFE_INTEGER),
      );
    return l;
  }, [apps, query, sort, locale, usage]);
  const idleDays = (id: string) => {
    const u = usage[id];
    return u ? Math.floor((Date.now() / 1000 - u) / 86_400) : null;
  };
  const app = apps?.find((a) => a.id === selected) ?? null;

  const uninstall = async (a: AppEntry) => {
    setAsk(null);
    setMsg(null);
    setWaiting(a.id);
    try {
      // Desktop apps return as soon as their uninstaller opens; Store apps once Windows removed them.
      await backend.appsUninstall(a.id);
      setWaiting(a.id);
      setMsg(t("apps.uninstalling", { name: a.name }));
    } catch (e) {
      setWaiting(null);
      setMsg(appsError(e, t, errorText));
    }
  };
  const protect = async (a: AppEntry) => {
    try {
      await backend.appsProtect(a.id, a.protected !== "user");
      await load();
    } catch (e) {
      setMsg(appsError(e, t, errorText));
    }
  };

  const luliData = useMemo(() => {
    if (!apps) return null;
    const rows = [...apps]
      .sort((a, b) => (b.size_bytes ?? 0) - (a.size_bytes ?? 0))
      .slice(0, 80)
      .map(
        (a) =>
          `- ${a.name} | ${a.publisher || "?"} | ${a.size_bytes ? sizeText(a.size_bytes) : "size ?"} | installed ${a.install_date ?? "?"}${a.hive === "store" ? " | Store" : ""}${a.protected ? ` | protected (${a.protected})` : ""}`,
      );
    return [`Installed apps (${apps.length}), largest first:`, ...rows].join("\n");
  }, [apps]);
  usePageData("apps", luliData);

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("apps.installed.title")}
          <HelpHint text={t("apps.installed.help")} />
        </span>
        {apps ? <Badge tone="neutral">{t("apps.count", { n: apps.length })}</Badge> : null}
        <span className="flex items-center gap-1.5 text-xs text-muted-foreground">
          <Upload className="size-3.5" />
          {t("apps.drop.hint")}
        </span>
        <label className="ml-auto flex h-9 w-60 items-center gap-2 rounded-md border border-border bg-background px-3 text-muted-foreground focus-within:border-ring">
          <Search className="size-4" />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={t("apps.search")}
            aria-label={t("apps.search")}
            className="w-full bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
          />
        </label>
        <div className="flex gap-1">
          {(["name", "size", "date", "use"] as const).map((k) => (
            <button
              key={k}
              type="button"
              onClick={() => setSort(k)}
              aria-pressed={sort === k}
              className={cn(
                "press rounded-md border px-2.5 py-1.5 text-xs font-semibold transition-colors duration-200 ease-out-expo",
                sort === k
                  ? "border-border-strong bg-foreground/8 text-foreground"
                  : "border-border text-muted-foreground hover:text-foreground",
              )}
            >
              {t(`apps.sort.${k}` as I18nKey)}
            </button>
          ))}
        </div>
        <Button variant="outline" disabled={!inTauri} onClick={load} aria-label={t("apps.refresh")}>
          <RefreshCw className="size-4" />
        </Button>
        {msg ? <p className="w-full text-xs text-foreground/90">{msg}</p> : null}
      </Card>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(320px,420px)_1fr]">
        <Card className="animate-rise min-w-0 py-2" style={rise(1)}>
          {apps === null && inTauri
            ? [0, 1, 2, 3, 4, 5].map((k) => <Skeleton key={k} className="my-2 h-12 w-full" />)
            : null}
          <ul className="max-h-160 space-y-0.5 overflow-y-auto pr-1">
            {rows.map((a) => (
              <li key={a.id}>
                <button
                  type="button"
                  onClick={() => setSelected(a.id)}
                  aria-pressed={a.id === selected}
                  className={cn(
                    "press flex w-full items-center gap-3 rounded-md px-3 py-2 text-left transition-colors duration-200 ease-out-expo",
                    a.id === selected ? "bg-foreground/7" : "hover:bg-foreground/4",
                  )}
                >
                  <AppIcon app={a} />
                  <span className="min-w-0 flex-1">
                    <span className="flex items-center gap-1.5">
                      <span className="truncate text-sm font-semibold">{a.name}</span>
                      {a.protected ? (
                        <Lock className="size-3 shrink-0 text-muted-foreground" />
                      ) : null}
                    </span>
                    <span className="block truncate text-[11px] text-muted-foreground">
                      {a.publisher || "—"}
                      {a.version ? ` · ${a.version}` : ""}
                      {sort === "use" && idleDays(a.id) !== null
                        ? ` · ${t("apps.used_ago", { n: idleDays(a.id) ?? 0 })}`
                        : ""}
                    </span>
                  </span>
                  <span className="num shrink-0 text-xs text-muted-foreground">
                    {a.size_bytes ? formatBytes(a.size_bytes, locale) : ""}
                  </span>
                </button>
              </li>
            ))}
          </ul>
          {apps && rows.length === 0 ? (
            <p className="py-8 text-center text-xs text-muted-foreground">{t("apps.empty")}</p>
          ) : null}
        </Card>

        <div className="min-w-0 space-y-4">
          {app ? (
            <Card className="animate-rise" style={rise(2)}>
              <div className="flex flex-wrap items-start gap-3">
                <AppIcon app={app} size="size-12" />
                <div className="min-w-0 flex-1">
                  <h3 className="font-display text-[17px] font-semibold tracking-wide">
                    {app.name}
                  </h3>
                  <p className="text-xs text-muted-foreground">
                    {[app.publisher, app.version, fmtDate(app.install_date, locale)]
                      .filter(Boolean)
                      .join(" · ")}
                  </p>
                  {app.install_location ? (
                    <p
                      className="mt-1 truncate font-mono text-[11px] text-muted-foreground/80"
                      title={app.install_location}
                    >
                      {app.install_location}
                    </p>
                  ) : null}
                  <div className="mt-2 flex flex-wrap gap-1.5">
                    {gone.has(app.id) ? <Badge tone="success">{t("apps.gone")}</Badge> : null}
                    {app.protected ? (
                      <Badge tone="neutral">
                        {t(`apps.protected.${app.protected}` as I18nKey)}
                      </Badge>
                    ) : null}
                    {app.needs_admin ? <Badge tone="neutral">{t("apps.all_users")}</Badge> : null}
                    {app.msi ? <Badge tone="neutral">MSI</Badge> : null}
                  </div>
                </div>
                <div className="flex gap-2">
                  {app.protected !== "runtime" &&
                  app.protected !== "self" &&
                  app.protected !== "security" ? (
                    <Button variant="ghost" onClick={() => protect(app)}>
                      {app.protected === "user" ? (
                        <ShieldOff className="size-4" />
                      ) : (
                        <Shield className="size-4" />
                      )}
                      {app.protected === "user" ? t("apps.unprotect") : t("apps.protect")}
                    </Button>
                  ) : null}
                  <Button
                    variant="danger"
                    disabled={!!app.protected || waiting !== null || gone.has(app.id)}
                    title={app.protected ? t("apps.error.protected") : undefined}
                    onClick={() => setAsk(app)}
                  >
                    <PackageX className="size-4" />
                    {waiting === app.id ? t("apps.waiting") : t("apps.uninstall")}
                  </Button>
                </div>
              </div>
            </Card>
          ) : (
            <Card className="animate-rise" style={rise(2)}>
              <EmptyState
                icon={<PackageX className="size-5" />}
                title={t("apps.pick.title")}
                body={t("apps.pick.body")}
              />
            </Card>
          )}
          {app ? (
            <Companions
              key={`c:${app.id}:${reload}`}
              app={app}
              busy={waiting !== null}
              onPick={(c) => {
                setSelected(c.id);
                setAsk(c);
              }}
            />
          ) : null}
          {app ? (
            <LeftoversPanel
              key={`${app.id}:${reload}`}
              appId={app.id}
              appName={app.name}
              installed={!gone.has(app.id)}
              recheck={reload}
              onRemoved={() => undefined}
            />
          ) : null}
        </div>
      </div>

      <Modal
        open={ask !== null}
        onClose={() => setAsk(null)}
        closeLabel={t("common.cancel")}
        title={t("apps.uninstall_confirm.title", { name: ask?.name ?? "" })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setAsk(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={() => ask && uninstall(ask)}>
              {t("apps.uninstall")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">
          {ask?.hive === "store"
            ? t("apps.uninstall_confirm.store")
            : t("apps.uninstall_confirm.desktop")}
        </p>
      </Modal>
    </>
  );
}

/** "Undo last removal": Recycle Bin items back to their places, registry backups re-imported. */
function UndoButton() {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [rec, setRec] = useState<UndoRecord | null>(null);
  const [msg, setMsg] = useState<string | null>(null);
  const refresh = useCallback(() => {
    backend.appsUndoStatus().then((r) => setRec(r));
  }, []);
  useEffect(() => {
    refresh();
    window.addEventListener(APPS_REMOVED, refresh);
    return () => window.removeEventListener(APPS_REMOVED, refresh);
  }, [refresh]);
  if (!rec && !msg) return null;
  return (
    <span className="flex items-center gap-2">
      {msg ? <span className="text-xs text-foreground/90">{msg}</span> : null}
      {rec ? (
        <Button
          variant="outline"
          size="sm"
          title={t("apps.undo.hint", {
            app: rec.app === SWEEP_ID ? t("apps.unowned.title") : rec.app,
            n: rec.files.length + rec.keys.length,
          })}
          onClick={async () => {
            try {
              const o = await backend.appsUndo();
              setMsg(t("apps.undo.done", { files: o.files, keys: o.keys, failed: o.failed }));
            } catch (e) {
              setMsg(errorText(e));
            }
            refresh();
          }}
        >
          <RotateCcw className="size-4" />
          {t("apps.undo", { app: rec.app === SWEEP_ID ? t("apps.unowned.title") : rec.app })}
        </Button>
      ) : null}
    </span>
  );
}

function OrphansTab({ autoScan, onAutoScanned }: { autoScan: boolean; onAutoScanned: () => void }) {
  const { t, locale } = useI18n();
  const [list, setList] = useState<OrphanApp[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<{ id: string; name: string } | null>(() =>
    boot.startTab === "unowned" ? { id: SWEEP_ID, name: t("apps.unowned.title") } : null,
  );
  const [reload, setReload] = useState(0);

  const scan = useCallback(async () => {
    setBusy(true);
    setSelected(null);
    // The orphan sweep compares with the installed list kept in the core: load it first.
    await backend.appsList();
    setList((await backend.appsOrphans()) ?? []);
    setBusy(false);
  }, []);
  // Opened from the "app removed" banner: sweep right away.
  useEffect(() => {
    if (!autoScan) return;
    onAutoScanned();
    void scan();
  }, [autoScan, onAutoScanned, scan]);

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("apps.orphans.title")}
          <HelpHint text={t("apps.orphans.help")} />
        </span>
        <Button variant="primary" className="ml-auto" disabled={busy || !inTauri} onClick={scan}>
          <RefreshCw className={cn("size-4", busy && "animate-spin")} />
          {t("apps.orphans.scan")}
        </Button>
      </Card>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4" style={rise(1)}>
        <HardDrive className="size-5 text-primary" />
        <span className="min-w-0 flex-1">
          <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
            {t("apps.unowned.title")}
            <HelpHint text={t("apps.unowned.help")} />
          </span>
          <span className="block text-xs text-muted-foreground">{t("apps.unowned.body")}</span>
        </span>
        <Button
          variant="outline"
          disabled={!inTauri}
          onClick={() => {
            setSelected({ id: SWEEP_ID, name: t("apps.unowned.title") });
            setReload((r) => r + 1);
          }}
        >
          <Search className="size-4" />
          {t("apps.unowned.scan")}
        </Button>
      </Card>
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[minmax(320px,420px)_1fr]">
        <Card className="animate-rise min-w-0 py-2" style={rise(1)}>
          {busy ? [0, 1, 2].map((k) => <Skeleton key={k} className="my-2 h-12 w-full" />) : null}
          {list && list.length === 0 && !busy ? (
            <p className="py-8 text-center text-xs text-muted-foreground">
              {t("apps.orphans.none")}
            </p>
          ) : null}
          {list === null && !busy ? (
            <p className="py-8 text-center text-xs text-muted-foreground">
              {t("apps.orphans.start")}
            </p>
          ) : null}
          <ul className="space-y-0.5">
            {(list ?? []).map((o) => (
              <li key={o.app.id}>
                <button
                  type="button"
                  onClick={() => setSelected({ id: o.app.id, name: o.app.name })}
                  aria-pressed={selected?.id === o.app.id}
                  className={cn(
                    "press flex w-full items-center gap-3 rounded-md px-3 py-2 text-left transition-colors duration-200 ease-out-expo",
                    selected?.id === o.app.id ? "bg-foreground/7" : "hover:bg-foreground/4",
                  )}
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-sm font-semibold">{o.app.name}</span>
                    <span className="block truncate text-[11px] text-muted-foreground">
                      {t("apps.orphans.items", { n: o.count })}
                    </span>
                  </span>
                  <span className="num shrink-0 text-xs text-muted-foreground">
                    {formatBytes(o.bytes, locale)}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </Card>
        <div className="min-w-0">
          {selected ? (
            <LeftoversPanel
              key={`${selected.id}:${reload}`}
              appId={selected.id}
              appName={selected.name}
              installed={false}
              recheck={reload}
              onRemoved={() => setReload((r) => r + 1)}
            />
          ) : null}
        </div>
      </div>
    </>
  );
}

function appsError(
  e: unknown,
  t: ReturnType<typeof useI18n>["t"],
  fallback: (e: unknown) => string,
) {
  const m = e instanceof Error ? e.message : String(e);
  if (m === "protected_app") return t("apps.error.protected");
  if (m.startsWith("app_running:")) return t("apps.error.running", { exe: m.slice(12) });
  if (m === "uninstaller_blocked") return t("apps.error.blocked");
  if (m === "uninstaller_missing") return t("apps.error.missing");
  if (m === "still_installed") return t("apps.error.still_installed");
  if (m === "unknown_app" || m === "unknown_item") return t("apps.error.stale");
  return fallback(e);
}

/** Leftovers of one app: reasons + confidence, checkboxes, confirmed removal (token in the core). */
/** Apps this one's installer added and that are still installed (Parsec → its drivers). */
function Companions({
  app,
  busy,
  onPick,
}: {
  app: AppEntry;
  busy: boolean;
  onPick: (c: AppEntry) => void;
}) {
  const { t } = useI18n();
  const [list, setList] = useState<AppEntry[]>([]);
  useEffect(() => {
    let alive = true;
    backend.appsCompanions(app.id).then((l) => alive && setList(l));
    return () => {
      alive = false;
    };
  }, [app.id]);
  if (list.length === 0) return null;
  return (
    <Card className="animate-rise border-warning/40">
      <CardTitle help={t("apps.companions.help")}>
        {t("apps.companions.title", { name: app.name })}
      </CardTitle>
      <ul className="space-y-2">
        {list.map((c) => (
          <li key={c.id} className="flex items-center gap-3">
            <AppIcon app={c} />
            <span className="min-w-0 flex-1">
              <span className="block truncate text-sm font-semibold">{c.name}</span>
              <span className="block truncate text-[11px] text-muted-foreground">
                {c.publisher}
              </span>
            </span>
            <Button
              size="sm"
              variant="outline"
              disabled={busy || !!c.protected}
              onClick={() => onPick(c)}
            >
              {t("apps.uninstall")}
            </Button>
          </li>
        ))}
      </ul>
    </Card>
  );
}

function LeftoversPanel({
  appId,
  appName,
  installed,
  recheck,
  onRemoved,
}: {
  appId: string;
  appName: string;
  installed: boolean;
  recheck: number;
  onRemoved: () => void;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [items, setItems] = useState<Leftover[] | null>(null);
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // biome-ignore lint/correctness/useExhaustiveDependencies: `recheck` re-runs the search after an uninstall
  useEffect(() => {
    let alive = true;
    setItems(null);
    backend.appsLeftovers(appId).then((l) => {
      if (!alive || !l) return;
      setItems(l);
      // The user's own data (saves, profiles, vaults) and near misses are never preselected.
      setPicked(new Set(l.filter((x) => x.confidence !== "low" && !x.precious).map((x) => x.id)));
    });
    return () => {
      alive = false;
    };
  }, [appId, recheck]);

  const chosen = (items ?? []).filter((x) => picked.has(x.id));
  const bytes = chosen.reduce((s, x) => s + x.bytes, 0);
  const toggle = (id: number) =>
    setPicked((cur) => {
      const n = new Set(cur);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const remove = async () => {
    setConfirm(false);
    setBusy(true);
    setMsg(null);
    try {
      const ids = chosen.map((x) => x.id);
      const token = await backend.appsConfirm(ids);
      const out: LeftoverOutcome = await backend.appsRemove(token.token, ids);
      setMsg(
        t("apps.removed", {
          files: out.recycled,
          size: formatBytes(out.bytes, locale),
          keys: out.registry + out.services,
        }) +
          (out.failed ? ` ${t("apps.removed_failed", { n: out.failed })}` : "") +
          (out.needs_admin ? ` ${t("apps.removed_admin", { n: out.needs_admin })}` : ""),
      );
      onRemoved();
      window.dispatchEvent(new Event(APPS_REMOVED));
    } catch (e) {
      setMsg(appsError(e, t, errorText));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Card className="animate-rise min-w-0" style={rise(3)}>
      <CardTitle help={t("apps.leftovers.help")}>
        {t("apps.leftovers.title")}
        {items ? <Badge tone="neutral">{items.length}</Badge> : null}
      </CardTitle>
      {installed ? (
        <p className="-mt-2 mb-3 text-xs text-muted-foreground">{t("apps.leftovers.preview")}</p>
      ) : null}
      {items === null
        ? [0, 1, 2, 3].map((k) => <Skeleton key={k} className="my-2 h-10 w-full" />)
        : null}
      {items && items.length === 0 ? (
        <p className="py-6 text-center text-xs text-muted-foreground">{t("apps.leftovers.none")}</p>
      ) : null}
      <ul className="divide-y divide-border/70">
        {(items ?? []).map((x) => {
          const Icon = KIND_ICON[x.kind];
          return (
            <li key={x.id} className="flex items-start gap-3 py-2.5">
              <input
                type="checkbox"
                checked={picked.has(x.id)}
                onChange={() => toggle(x.id)}
                aria-label={x.path}
                className="mt-1 size-4 accent-primary"
              />
              <Icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
              <div className="min-w-0 flex-1">
                <p className="truncate font-mono text-[12px]" title={x.path}>
                  {x.path}
                </p>
                <p className="mt-0.5 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                  <Badge tone={CONF_TONE[x.confidence]}>
                    {t(`apps.conf.${x.confidence}` as I18nKey)}
                  </Badge>
                  {t(`apps.reason.${x.reason}` as I18nKey)}
                  {x.idle_days !== undefined ? (
                    <span>{t("apps.idle", { n: x.idle_days })}</span>
                  ) : null}
                  {x.precious ? <Badge tone="warning">{t("apps.precious")}</Badge> : null}
                  {x.needs_admin ? (
                    <Badge tone="neutral">{t("common.admin_required")}</Badge>
                  ) : null}
                </p>
              </div>
              <span className="num shrink-0 text-xs text-muted-foreground">
                {x.bytes ? formatBytes(x.bytes, locale) : ""}
              </span>
            </li>
          );
        })}
      </ul>
      {items && items.length > 0 ? (
        <div className="mt-3 flex flex-wrap items-center gap-3 border-t border-border pt-3">
          <span className="text-xs text-muted-foreground">
            {t("apps.selected", { n: chosen.length, size: formatBytes(bytes, locale) })}
          </span>
          <Button
            variant="danger"
            className="ml-auto"
            disabled={installed || chosen.length === 0 || busy}
            title={installed ? t("apps.error.still_installed") : undefined}
            onClick={() => setConfirm(true)}
          >
            <Trash2 className="size-4" />
            {t("apps.remove", { n: chosen.length })}
          </Button>
        </div>
      ) : null}
      {msg ? <p className="mt-2 text-xs text-foreground/90">{msg}</p> : null}

      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        closeLabel={t("common.cancel")}
        title={t("apps.confirm.title", { n: chosen.length, name: appName })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={remove}>
              {t("apps.remove", { n: chosen.length })}
            </Button>
          </>
        }
      >
        {chosen.some((x) => x.precious) ? (
          <p className="mb-3 rounded-md bg-warning/10 px-3 py-2 text-warning ring-1 ring-inset ring-warning/25">
            {t("apps.confirm.precious")}
          </p>
        ) : null}
        <ul className="list-disc space-y-1 pl-5 text-muted-foreground">
          <li>{t("apps.confirm.files")}</li>
          <li>{t("apps.confirm.registry")}</li>
          <li>{t("apps.confirm.services")}</li>
        </ul>
      </Modal>
    </Card>
  );
}
