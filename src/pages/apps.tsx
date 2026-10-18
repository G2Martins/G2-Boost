import {
  Cog,
  Database,
  File as FileIcon,
  Folder,
  Link2,
  Lock,
  PackageX,
  RefreshCw,
  Search,
  Shield,
  ShieldOff,
  Trash2,
  Upload,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
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
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { onDrop, takeDrop } from "@/lib/nav";
import { cn, formatBytes } from "@/lib/utils";

const KIND_ICON = {
  folder: Folder,
  file: FileIcon,
  shortcut: Link2,
  registry: Database,
  service: Cog,
} as const;

const CONF_TONE = { high: "success", medium: "warning", low: "neutral" } as const;

export function AppsPage() {
  const { t } = useI18n();
  const [tab, setTab] = useState("installed");
  const [drop, setDrop] = useState<string | null>(() => takeDrop());
  useEffect(
    () =>
      onDrop((p) => {
        setTab("installed");
        setDrop(p);
      }),
    [],
  );
  return (
    <PageHeader
      title={t("apps.title")}
      subtitle={t("apps.subtitle")}
      tabs={
        <Tabs value={tab} onValueChange={setTab}>
          <TabsList>
            <TabsTrigger value="installed">{t("apps.tab.installed")}</TabsTrigger>
            <TabsTrigger value="orphans">{t("apps.tab.orphans")}</TabsTrigger>
          </TabsList>
          <TabsContent value="installed">
            <PageBody className="px-0 pb-0">
              <InstalledTab drop={drop} onDropHandled={() => setDrop(null)} />
            </PageBody>
          </TabsContent>
          <TabsContent value="orphans">
            <PageBody className="px-0 pb-0">
              <OrphansTab />
            </PageBody>
          </TabsContent>
        </Tabs>
      }
    />
  );
}

function fmtDate(d: string | null, locale: string) {
  if (!d || !/^\d{8}$/.test(d)) return null;
  const date = new Date(Number(d.slice(0, 4)), Number(d.slice(4, 6)) - 1, Number(d.slice(6, 8)));
  return date.toLocaleDateString(locale === "en" ? "en-US" : "pt-BR");
}

function InstalledTab({ drop, onDropHandled }: { drop: string | null; onDropHandled: () => void }) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [apps, setApps] = useState<AppEntry[] | null>(null);
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState<"name" | "size" | "date">("name");
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
    return l;
  }, [apps, query, sort, locale]);
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
          {(["name", "size", "date"] as const).map((k) => (
            <button
              key={k}
              type="button"
              onClick={() => setSort(k)}
              aria-pressed={sort === k}
              className={cn(
                "press rounded-md border px-2.5 py-1.5 text-xs font-semibold transition-colors duration-200 ease-out-expo",
                sort === k
                  ? "border-border-strong bg-foreground/[0.08] text-foreground"
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
          <ul className="max-h-[640px] space-y-0.5 overflow-y-auto pr-1">
            {rows.map((a) => (
              <li key={a.id}>
                <button
                  type="button"
                  onClick={() => setSelected(a.id)}
                  aria-pressed={a.id === selected}
                  className={cn(
                    "press flex w-full items-center gap-3 rounded-md px-3 py-2 text-left transition-colors duration-200 ease-out-expo",
                    a.id === selected ? "bg-foreground/[0.07]" : "hover:bg-foreground/[0.04]",
                  )}
                >
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

function OrphansTab() {
  const { t, locale } = useI18n();
  const [list, setList] = useState<OrphanApp[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [selected, setSelected] = useState<OrphanApp | null>(null);
  const [reload, setReload] = useState(0);

  const scan = async () => {
    setBusy(true);
    setSelected(null);
    // The orphan sweep compares with the installed list kept in the core: load it first.
    await backend.appsList();
    setList((await backend.appsOrphans()) ?? []);
    setBusy(false);
  };

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
                  onClick={() => setSelected(o)}
                  aria-pressed={selected?.app.id === o.app.id}
                  className={cn(
                    "press flex w-full items-center gap-3 rounded-md px-3 py-2 text-left transition-colors duration-200 ease-out-expo",
                    selected?.app.id === o.app.id
                      ? "bg-foreground/[0.07]"
                      : "hover:bg-foreground/[0.04]",
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
              key={`${selected.app.id}:${reload}`}
              appId={selected.app.id}
              appName={selected.app.name}
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
  if (m === "uninstaller_blocked") return t("apps.error.blocked");
  if (m === "uninstaller_missing") return t("apps.error.missing");
  if (m === "still_installed") return t("apps.error.still_installed");
  if (m === "unknown_app" || m === "unknown_item") return t("apps.error.stale");
  return fallback(e);
}

/** Leftovers of one app: reasons + confidence, checkboxes, confirmed removal (token in the core). */
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
      setPicked(new Set(l.filter((x) => x.confidence !== "low").map((x) => x.id)));
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
                className="mt-1 size-4 accent-[hsl(var(--primary))]"
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
        <ul className="list-disc space-y-1 pl-5 text-muted-foreground">
          <li>{t("apps.confirm.files")}</li>
          <li>{t("apps.confirm.registry")}</li>
          <li>{t("apps.confirm.services")}</li>
        </ul>
      </Modal>
    </Card>
  );
}
