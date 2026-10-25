import {
  Check,
  CircleArrowUp,
  Clapperboard,
  Code,
  Cpu,
  Download,
  FileText,
  Gamepad2,
  Globe,
  History,
  Layers,
  type LucideIcon,
  MessagesSquare,
  Network,
  PackagePlus,
  PenTool,
  RefreshCw,
  Search,
  Server,
  ShieldAlert,
  ShieldCheck,
  Square,
  Star,
  Trash2,
  Wrench,
} from "lucide-react";
import {
  type CSSProperties,
  type ReactNode,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { createPortal } from "react-dom";
import { Rail } from "@/components/apps/updates-tab";
import { rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  HelpHint,
  Modal,
  Skeleton,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import {
  backend,
  type InstallBatch,
  type InstallCategory,
  type InstallEntry,
  type InstallProgress,
  type InstallState,
  type InstallStatus,
  inTauri,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/* ------------------------------------------------------------------------------------------------
 * Module store: a batch runs for minutes, and the selection is worth keeping while the user looks
 * at another tab or page (same idea as the updates tab and the console blocks).
 * ---------------------------------------------------------------------------------------------- */

/** Mirrors `install::MAX_BATCH` in the core, which refuses a longer list anyway. */
const MAX_BATCH = 60;

type Run = {
  ids: string[];
  progress: Record<string, InstallProgress>;
  result: InstallBatch | null;
  stopping: boolean;
};
type Store = {
  catalog: InstallEntry[] | null;
  status: InstallStatus | null;
  statusAt: number;
  statusBusy: boolean;
  /** Ticked ids, in the order they were ticked: the order they install in. */
  picked: string[];
  run: Run | null;
  error: string | null;
  runs: InstallBatch[];
};
let store: Store = {
  catalog: null,
  status: null,
  statusAt: 0,
  statusBusy: false,
  picked: [],
  run: null,
  error: null,
  runs: [],
};
const listeners = new Set<() => void>();
const set = (patch: Partial<Store>) => {
  store = { ...store, ...patch };
  for (const l of listeners) l();
};
const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => {
    listeners.delete(l);
  };
};
const useStore = () => useSyncExternalStore(subscribe, () => store);
const errCode = (e: unknown) => (e instanceof Error ? e.message : String(e));

async function loadCatalog() {
  const catalog = await backend.installCatalog();
  if (catalog) set({ catalog });
}

/** A refresh asked for while one is running (the batch just ended): run it right after. */
let refreshAgain = false;

async function loadStatus(refresh: boolean) {
  if (store.statusBusy) {
    refreshAgain ||= refresh;
    return;
  }
  set({ statusBusy: true });
  const status = await backend.installStatus(refresh);
  // Whatever turned out to be installed cannot stay ticked.
  const installed = new Set((status?.states ?? []).filter((x) => x.installed).map((x) => x.id));
  set({
    status: status ?? store.status,
    statusAt: Date.now(),
    statusBusy: false,
    picked: store.picked.filter((id) => !installed.has(id)),
  });
  if (refreshAgain) {
    refreshAgain = false;
    void loadStatus(true);
  }
}

async function loadRuns() {
  set({ runs: (await backend.installHistory()) ?? [] });
}

/** false when the list is already at the core's limit. */
function toggle(id: string): boolean {
  if (store.picked.includes(id)) {
    set({ picked: store.picked.filter((x) => x !== id) });
    return true;
  }
  if (store.picked.length >= MAX_BATCH) return false;
  set({ picked: [...store.picked, id] });
  return true;
}

function pickMany(ids: string[]) {
  const next = [...store.picked];
  for (const id of ids) if (!next.includes(id) && next.length < MAX_BATCH) next.push(id);
  set({ picked: next });
}

async function startRun(ids: string[]) {
  set({ run: { ids, progress: {}, result: null, stopping: false }, error: null });
  const off = await backend.on<InstallProgress>("install:progress", (p) => {
    const r = store.run;
    if (r) set({ run: { ...r, progress: { ...r.progress, [p.id]: p } } });
  });
  try {
    // The token is bound to this exact, ordered list.
    const token = await backend.installConfirm(ids);
    const result = await backend.installRun(token.token, ids);
    const done = new Set(result.items.filter((i) => i.status === "ok").map((i) => i.id));
    const r = store.run;
    // What did not install stays ticked, ready for another try.
    if (r) set({ run: { ...r, result }, picked: store.picked.filter((id) => !done.has(id)) });
  } catch (e) {
    set({ error: errCode(e), run: null });
  } finally {
    off();
    void loadStatus(true);
    void loadRuns();
  }
}

/* ------------------------------------------------------------------------------------------------
 * Static look of each category: icon and the token its monogram plate is tinted with.
 * ---------------------------------------------------------------------------------------------- */

const CATEGORIES: readonly { id: InstallCategory; icon: LucideIcon; tint: string }[] = [
  { id: "browsers", icon: Globe, tint: "--class-code" },
  { id: "communication", icon: MessagesSquare, tint: "--class-audio" },
  { id: "games", icon: Gamepad2, tint: "--class-binary" },
  { id: "media", icon: Clapperboard, tint: "--class-archive" },
  { id: "creative", icon: PenTool, tint: "--chart-2" },
  { id: "dev", icon: Code, tint: "--chart-3" },
  { id: "utilities", icon: Wrench, tint: "--class-other" },
  { id: "office", icon: FileText, tint: "--class-document" },
  { id: "security", icon: ShieldCheck, tint: "--class-audio" },
  { id: "hardware", icon: Cpu, tint: "--class-binary" },
  { id: "network", icon: Network, tint: "--class-code" },
  { id: "self_hosted", icon: Server, tint: "--class-archive" },
];
const LOOK = Object.fromEntries(CATEGORIES.map((c) => [c.id, c])) as Record<
  InstallCategory,
  (typeof CATEGORIES)[number]
>;

const fold = (s: string) => s.normalize("NFD").replace(/\p{M}/gu, "").toLowerCase();

const STOP_WORDS = new Set(["of", "the", "and", "for", "de", "da", "do", "e", "para"]);

/** Two letters for the plate: `Visual Studio Code` → VS, `7-Zip` → 7Z, `Steam` → ST. */
export function monogram(name: string): string {
  const words = name
    .replace(/\([^)]*\)/g, " ")
    .split(/[^\p{L}\p{N}]+/u)
    .filter((w) => w && !STOP_WORDS.has(w.toLowerCase()));
  const [a, b] = words;
  if (!a) return "?";
  return (b ? `${a.charAt(0)}${b.charAt(0)}` : a.slice(0, 2)).toUpperCase();
}

/** One request per installed app for the whole session. */
const icons = new Map<string, Promise<string | null>>();
function iconFor(appId: string) {
  let hit = icons.get(appId);
  if (!hit) {
    hit = backend.appsIcon(appId);
    icons.set(appId, hit);
  }
  return hit;
}

/** Plain plural without a library: the dictionary holds both forms. */
const many = (n: number) => (n === 1 ? "one" : "other");

const SKELETONS = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"];

/* ------------------------------------------------------------------------------------------------ */

export function InstallTab({
  onUninstall,
  onOpenUpdates,
}: {
  /** Hand an installed app to the native uninstaller (the Installed tab). */
  onUninstall: (appId: string) => void;
  onOpenUpdates: () => void;
}) {
  const { t } = useI18n();
  const s = useStore();
  const [query, setQuery] = useState("");
  const [cat, setCat] = useState<"all" | InstallCategory>("all");
  const [onlyPicked, setOnlyPicked] = useState(false);
  const [confirm, setConfirm] = useState(false);
  // Assumed until known, so the administrator note in the dialog never flashes in wrongly.
  const [elevated, setElevated] = useState(true);

  useEffect(() => {
    backend.appInfo().then((i) => i && setElevated(i.elevated));
    if (!inTauri) return;
    if (store.catalog === null) void loadCatalog();
    if (!store.run && (store.status === null || Date.now() - store.statusAt > 30_000))
      void loadStatus(false);
    void loadRuns();
  }, []);

  const run = s.run;
  const running = run !== null && run.result === null;
  const wingetMissing = s.status !== null && !s.status.winget;
  const locked = running || wingetMissing;
  // The action bar shows for a selection and for the batch in progress.
  const dockOpen = s.picked.length > 0 || running;

  const states = useMemo(() => new Map((s.status?.states ?? []).map((x) => [x.id, x])), [s.status]);
  const pickedSet = useMemo(() => new Set(s.picked), [s.picked]);
  const names = useMemo(() => new Map((s.catalog ?? []).map((e) => [e.id, e.name])), [s.catalog]);

  // What the search matches: name, id and the category's label in the current language.
  const haystack = useMemo(
    () =>
      new Map(
        (s.catalog ?? []).map((e) => [
          e.id,
          fold(`${e.name} ${e.id} ${t(`install.cat.${e.category}` as I18nKey)}`),
        ]),
      ),
    [s.catalog, t],
  );
  const q = fold(query.trim());
  const matching = useMemo(
    () => (s.catalog ?? []).filter((e) => !q || haystack.get(e.id)?.includes(q)),
    [s.catalog, haystack, q],
  );
  const visible = useMemo(
    () =>
      matching.filter(
        (e) => (cat === "all" || e.category === cat) && (!onlyPicked || pickedSet.has(e.id)),
      ),
    [matching, cat, onlyPicked, pickedSet],
  );
  const groups = useMemo(
    () =>
      CATEGORIES.map((c) => ({ ...c, items: visible.filter((e) => e.category === c.id) })).filter(
        (g) => g.items.length > 0,
      ),
    [visible],
  );
  const perCategory = useMemo(() => {
    const n = new Map<string, number>();
    for (const e of matching) n.set(e.category, (n.get(e.category) ?? 0) + 1);
    return n;
  }, [matching]);

  const updates = (s.status?.states ?? []).filter((x) => x.installed && x.update).length;
  const showOnlyPicked = onlyPicked && s.picked.length > 0;

  // One toast when a batch ends while this tab is on screen.
  const announced = useRef(run?.result ?? null);
  useEffect(() => {
    const r = run?.result;
    if (!r || r === announced.current) return;
    announced.current = r;
    const c = counts(r);
    toast(t("install.run.done", c), { tone: c.failed ? "warning" : "success" });
  }, [run?.result, t]);

  const pick = (id: string) => {
    if (!toggle(id)) toast(t("install.max", { n: MAX_BATCH }), { tone: "warning" });
  };

  const pickEssentials = () => {
    const missing = (s.catalog ?? []).filter((e) => e.essential && !states.get(e.id)?.installed);
    if (missing.length === 0) return toast(t("install.essentials.none"));
    pickMany(missing.map((e) => e.id));
    toast(t("install.essentials.done", { n: missing.length }), { tone: "success" });
  };

  const chosen = s.picked.map((id) => ({ id, name: names.get(id) ?? id }));

  return (
    <div className={cn("space-y-4", dockOpen && "pb-28")}>
      <Card className="animate-rise space-y-3 py-4">
        <div className="flex flex-wrap items-center gap-3">
          <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
            <PackagePlus className="size-4 text-primary" />
            {t("install.title")}
            <HelpHint text={t("install.help")} />
          </span>
          {s.catalog ? (
            <Badge tone="neutral">{t("install.count", { n: s.catalog.length })}</Badge>
          ) : null}
          <label className="ml-auto flex h-9 w-64 items-center gap-2 rounded-md border border-border bg-background px-3 text-muted-foreground focus-within:border-ring">
            <Search className="size-4" />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder={t("install.search")}
              aria-label={t("install.search")}
              className="w-full bg-transparent text-[13px] text-foreground outline-none placeholder:text-muted-foreground"
            />
          </label>
          <Button
            variant="outline"
            disabled={!s.catalog || locked}
            title={t("install.essentials.hint")}
            onClick={pickEssentials}
          >
            <Star className="size-4" />
            {t("install.essentials")}
          </Button>
          <Button
            variant="outline"
            disabled={!inTauri || s.statusBusy || running}
            aria-label={t("install.refresh")}
            title={t("install.refresh")}
            onClick={() => void loadStatus(true)}
          >
            <RefreshCw className={cn("size-4", s.statusBusy && "animate-spin")} />
          </Button>
        </div>

        <fieldset
          aria-label={t("install.categories")}
          className="m-0 flex min-w-0 flex-wrap gap-1.5 border-0 p-0"
        >
          <Chip
            active={cat === "all"}
            icon={<Layers className="size-3.5" />}
            count={s.catalog ? matching.length : undefined}
            onClick={() => setCat("all")}
          >
            {t("install.cat.all")}
          </Chip>
          {CATEGORIES.map((c) => (
            <Chip
              key={c.id}
              active={cat === c.id}
              icon={<c.icon className="size-3.5" />}
              count={s.catalog ? (perCategory.get(c.id) ?? 0) : undefined}
              onClick={() => setCat(cat === c.id ? "all" : c.id)}
            >
              {t(`install.cat.${c.id}` as I18nKey)}
            </Chip>
          ))}
          <Chip
            accent
            active={showOnlyPicked}
            disabled={s.picked.length === 0}
            title={t("install.selected.hint")}
            icon={<Check className="size-3.5" strokeWidth={3} />}
            className="ml-auto"
            onClick={() => setOnlyPicked(!onlyPicked)}
          >
            {t("install.selected", { n: s.picked.length })}
          </Chip>
        </fieldset>

        {s.statusBusy && !s.status ? (
          <div className="flex items-center gap-3">
            <Rail percent={null} />
            <span className="shrink-0 text-xs text-muted-foreground">{t("install.checking")}</span>
          </div>
        ) : null}
      </Card>

      {wingetMissing ? (
        <p className="flex items-start gap-2 rounded-md bg-warning/8 px-3 py-2 text-xs text-foreground/90 ring-1 ring-warning/25 ring-inset">
          <ShieldAlert className="mt-0.5 size-3.5 shrink-0 text-warning" />
          <span>
            <strong className="font-semibold">{t("install.winget.title")}.</strong>{" "}
            {t("install.winget.body")}
          </span>
        </p>
      ) : null}

      {updates > 0 ? (
        <p className="flex flex-wrap items-center gap-2 rounded-md bg-foreground/4 px-3 py-1.5 text-xs text-foreground/90 ring-1 ring-border ring-inset">
          <CircleArrowUp className="size-4 shrink-0 text-warning" />
          <span>{t(`install.updates.banner.${many(updates)}` as I18nKey, { n: updates })}</span>
          <Button variant="ghost" size="sm" className="ml-auto" onClick={onOpenUpdates}>
            {t("install.updates.all")}
          </Button>
        </p>
      ) : null}

      {s.error ? <p className="text-xs text-destructive">{errorText(s.error, t)}</p> : null}

      {run ? <RunCard run={run} names={names} onClose={() => set({ run: null })} /> : null}

      {s.catalog === null && inTauri ? (
        <div className="grid grid-cols-[repeat(auto-fill,minmax(232px,1fr))] gap-2">
          {SKELETONS.map((k) => (
            <Skeleton key={k} className="h-16 w-full" />
          ))}
        </div>
      ) : null}
      {s.catalog === null && !inTauri ? (
        <Card className="animate-rise" style={rise(1)}>
          <EmptyState
            icon={<PackagePlus className="size-5" />}
            title={t("install.title")}
            body={t("install.unavailable")}
          />
        </Card>
      ) : null}
      {s.catalog && groups.length === 0 ? (
        <p className="py-8 text-center text-xs text-muted-foreground">{t("install.empty")}</p>
      ) : null}

      {groups.map((g, gi) => (
        <Section
          key={g.id}
          index={gi}
          icon={<g.icon className="size-4 text-muted-foreground" />}
          title={t(`install.cat.${g.id}` as I18nKey)}
          count={g.items.length}
        >
          {g.items.map((e) => {
            const st = states.get(e.id);
            return st?.installed ? (
              <InstalledTile key={e.id} entry={e} state={st} onUninstall={onUninstall} />
            ) : (
              <PickTile
                key={e.id}
                entry={e}
                state={st}
                picked={pickedSet.has(e.id)}
                locked={locked}
                onToggle={pick}
              />
            );
          })}
        </Section>
      ))}

      <HistoryCard runs={s.runs} />

      {dockOpen ? (
        <Dock label={t("install.title")}>
          {running && run ? (
            <RunDock run={run} names={names} />
          ) : (
            <div className="flex items-center gap-4">
              <div
                className="flex items-baseline gap-2"
                aria-live="polite"
                title={t("install.dock.hint")}
              >
                <span className="num font-display text-3xl leading-none font-semibold">
                  {s.picked.length}
                </span>
                <span className="text-sm font-semibold">
                  {t(`install.dock.${many(s.picked.length)}` as I18nKey)}
                </span>
              </div>
              <div className="ml-auto flex items-center gap-2">
                <Button variant="ghost" onClick={() => set({ picked: [] })}>
                  {t("install.dock.clear")}
                </Button>
                <Button
                  variant="primary"
                  size="lg"
                  tactile
                  disabled={!inTauri || wingetMissing || s.picked.length === 0}
                  onClick={() => setConfirm(true)}
                >
                  <Download className="size-4" />
                  {t("install.dock.go")}
                </Button>
              </div>
            </div>
          )}
        </Dock>
      ) : null}

      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        closeLabel={t("common.cancel")}
        title={t(`install.confirm.title.${many(chosen.length)}` as I18nKey, { n: chosen.length })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                setConfirm(false);
                setOnlyPicked(false);
                void startRun(chosen.map((c) => c.id));
              }}
            >
              <Download className="size-4" />
              {t("install.confirm.go")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("install.confirm.body")}</p>
        {!elevated ? (
          <p className="mt-3 rounded-md bg-warning/10 px-3 py-2 text-warning ring-1 ring-warning/25 ring-inset">
            {t("install.confirm.uac")}
          </p>
        ) : null}
        <ol className="mt-3 max-h-56 list-decimal space-y-0.5 overflow-y-auto pl-6 text-muted-foreground marker:text-muted-foreground/60">
          {chosen.map((c) => (
            <li key={c.id}>
              <span className="text-foreground/90">{c.name}</span>{" "}
              <span className="font-mono text-[11px]">{c.id}</span>
            </li>
          ))}
        </ol>
      </Modal>
    </div>
  );
}

/* ---------------- pieces ---------------- */

const counts = (b: InstallBatch) => ({
  ok: b.items.filter((i) => i.status === "ok").length,
  failed: b.items.filter((i) => i.status === "failed").length,
  cancelled: b.items.filter((i) => i.status === "cancelled").length,
});

function errorText(code: string, t: ReturnType<typeof useI18n>["t"]) {
  if (code === "busy") return t("install.error.busy");
  if (code === "bad_selection" || code === "bad_token") return t("install.error.bad_selection");
  if (code === "winget_unavailable") return t("updates.error.winget_missing");
  return t("boost.error.generic", { error: code });
}

function Chip({
  active,
  accent,
  icon,
  count,
  disabled,
  title,
  className,
  onClick,
  children,
}: {
  active: boolean;
  accent?: boolean;
  icon?: ReactNode;
  count?: number;
  disabled?: boolean;
  title?: string;
  className?: string;
  onClick: () => void;
  children: ReactNode;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      disabled={disabled}
      title={title}
      onClick={onClick}
      className={cn(
        "press inline-flex items-center gap-1.5 rounded-md border px-2.5 py-1.5 text-xs font-semibold transition-colors duration-200 ease-out-expo disabled:pointer-events-none disabled:opacity-40",
        active
          ? accent
            ? "border-primary/50 bg-primary/12 text-primary"
            : "border-border-strong bg-foreground/8 text-foreground"
          : "border-border text-muted-foreground hover:text-foreground",
        className,
      )}
    >
      {icon}
      {children}
      {count !== undefined ? (
        <span className="num text-[10px] font-medium text-muted-foreground">{count}</span>
      ) : null}
    </button>
  );
}

function Section({
  index,
  icon,
  title,
  count,
  children,
}: {
  index: number;
  icon: ReactNode;
  title: string;
  count: number;
  children: ReactNode;
}) {
  const id = useId();
  return (
    <section
      aria-labelledby={id}
      className="animate-rise space-y-2.5"
      style={rise(Math.min(index + 1, 6))}
    >
      <h3
        id={id}
        className="flex items-center gap-2 font-display text-[13px] font-semibold tracking-wide"
      >
        {icon}
        {title}
        <span className="num text-[11px] font-normal text-muted-foreground">{count}</span>
        <span aria-hidden className="rule-accent ml-1 flex-1" />
      </h3>
      <ul className="grid grid-cols-[repeat(auto-fill,minmax(232px,1fr))] gap-2">{children}</ul>
    </section>
  );
}

/**
 * The tile's picture: the installed app's own icon, or a monogram plate tinted by its category
 * (no network, no emoji) with the category's icon on a small chip. A dot marks installed apps.
 */
function Glyph({
  entry,
  state,
  size = "md",
}: {
  entry: InstallEntry;
  state?: InstallState;
  size?: "md" | "sm";
}) {
  const look = LOOK[entry.category];
  const Icon = look.icon;
  const appId = state?.installed ? (state.app ?? null) : null;
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    let alive = true;
    setSrc(null);
    if (appId) void iconFor(appId).then((s) => alive && setSrc(s));
    return () => {
      alive = false;
    };
  }, [appId]);
  const big = size === "md";
  const tint = { "--tint": `var(${look.tint})` } as CSSProperties;
  return (
    <span
      aria-hidden
      style={tint}
      className={cn("relative grid shrink-0 place-items-center", big ? "size-10" : "size-7")}
    >
      {src ? (
        <span
          className={cn(
            "grid size-full place-items-center bg-foreground/5 ring-1 ring-foreground/10 ring-inset",
            big ? "rounded-lg" : "rounded-md",
          )}
        >
          <img
            src={src}
            alt=""
            draggable={false}
            className={cn("object-contain", big ? "size-7" : "size-5")}
          />
        </span>
      ) : (
        <span
          className={cn("grid size-full place-items-center", big ? "rounded-lg" : "rounded-md")}
          style={{
            backgroundImage:
              "linear-gradient(150deg, hsl(var(--tint) / 0.26), hsl(var(--tint) / 0.07))",
            boxShadow:
              "inset 0 0 0 1px hsl(var(--tint) / 0.32), inset 0 1px 0 hsl(var(--foreground) / 0.12)",
          }}
        >
          <span
            className={cn(
              "font-display font-semibold tracking-wide text-foreground/90",
              big ? "text-[13px]" : "text-[10px]",
            )}
          >
            {monogram(entry.name)}
          </span>
        </span>
      )}
      {big && !src ? (
        <span className="absolute -right-1 -bottom-1 grid size-4 place-items-center rounded-full bg-background ring-1 ring-border-strong">
          <Icon className="size-2.5" strokeWidth={2.25} style={{ color: "hsl(var(--tint))" }} />
        </span>
      ) : null}
      {state?.installed ? (
        <span
          className={cn(
            "absolute -top-1 -right-1 size-2.5 rounded-full ring-2 ring-background",
            state.update
              ? "bg-warning shadow-[0_0_8px_hsl(var(--warning)/0.7)]"
              : "bg-success shadow-[0_0_8px_hsl(var(--success)/0.7)]",
          )}
        />
      ) : null}
    </span>
  );
}

function PickTile({
  entry,
  state,
  picked,
  locked,
  onToggle,
}: {
  entry: InstallEntry;
  state: InstallState | undefined;
  picked: boolean;
  locked: boolean;
  onToggle: (id: string) => void;
}) {
  const uid = useId();
  return (
    <li>
      <label
        htmlFor={uid}
        title={entry.name}
        className={cn(
          "panel panel-hover group relative flex min-h-16 items-center gap-3 overflow-clip rounded-lg px-3 py-2.5 select-none has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-ring",
          picked &&
            "bg-linear-to-br from-primary/12 to-transparent outline-1 -outline-offset-1 outline-primary/70",
          locked ? "cursor-default opacity-60" : "cursor-pointer",
        )}
      >
        <input
          id={uid}
          type="checkbox"
          className="sr-only scroll-mb-28"
          checked={picked}
          disabled={locked}
          onChange={() => onToggle(entry.id)}
          aria-label={entry.name}
        />
        <Glyph entry={entry} state={state} />
        <span className="line-clamp-2 min-w-0 flex-1 text-[13px] leading-tight font-semibold break-words">
          {entry.name}
        </span>
        <span
          aria-hidden
          className={cn(
            "grid size-5 shrink-0 place-items-center rounded-md border transition-colors duration-200 ease-out-expo",
            picked
              ? "border-primary bg-primary text-primary-foreground"
              : "border-border-strong bg-background/40 text-transparent group-hover:border-muted-foreground",
          )}
        >
          <Check
            className={cn(
              "size-3.5 motion-safe:transition-transform motion-safe:duration-200",
              picked ? "scale-100" : "scale-50",
            )}
            strokeWidth={3}
          />
        </span>
      </label>
    </li>
  );
}

function InstalledTile({
  entry,
  state,
  onUninstall,
}: {
  entry: InstallEntry;
  state: InstallState;
  onUninstall: (appId: string) => void;
}) {
  const { t } = useI18n();
  const app = state.app;
  const version = state.version ?? "?";
  const hint = state.update
    ? t("install.tile.update_hint", { version, available: state.update })
    : state.version
      ? t("install.tile.installed_hint", { version })
      : t("install.tile.installed");
  return (
    <li>
      <div
        title={`${entry.name}. ${hint}`}
        className="panel group relative flex min-h-16 items-center gap-3 rounded-lg px-3 py-2.5"
      >
        <Glyph entry={entry} state={state} />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] leading-tight font-semibold">
            {entry.name}
          </span>
          <span
            className={cn(
              "mt-1 flex items-center gap-1 text-[11px] font-medium",
              state.update ? "text-warning" : "text-success",
            )}
          >
            {state.update ? <CircleArrowUp className="size-3 shrink-0" /> : null}
            <span className="truncate">
              {state.update ? t("install.tile.update") : t("install.tile.installed")}
            </span>
          </span>
        </span>
        {app ? (
          <button
            type="button"
            onClick={() => onUninstall(app)}
            aria-label={`${t("install.uninstall")}: ${entry.name}`}
            title={t("install.uninstall.hint")}
            className="press -mr-1 grid size-8 shrink-0 place-items-center rounded-md text-muted-foreground opacity-0 transition-[opacity,color,background-color] duration-200 ease-out-expo group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-foreground/6 hover:text-destructive focus-visible:opacity-100"
          >
            <Trash2 className="size-4" />
          </button>
        ) : null}
      </div>
    </li>
  );
}

/**
 * The action bar. The page header clips its children (`overflow-hidden`), which would stop a
 * `sticky` bar from ever sticking, so it is mounted at the end of the shell's scroll area instead:
 * a zero-height sticky anchor with the bar floating above its bottom edge.
 */
function Dock({ label, children }: { label: string; children: ReactNode }) {
  const [host, setHost] = useState<HTMLElement | null>(null);
  useLayoutEffect(() => setHost(document.querySelector("main")), []);
  if (!host) return null;
  return createPortal(
    <div className="pointer-events-none sticky bottom-0 z-30 h-0">
      <div className="absolute inset-x-0 bottom-4 flex justify-start px-6">
        <section
          aria-label={label}
          className="animate-rise pointer-events-auto w-full max-w-[640px] rounded-xl border border-border-strong bg-surface/95 p-3 pl-4 shadow-[0_18px_40px_-14px_hsl(0_0%_0%/0.75),inset_0_1px_0_hsl(var(--foreground)/0.06)] backdrop-blur"
        >
          {children}
        </section>
      </div>
    </div>,
    host,
  );
}

function RunDock({ run, names }: { run: Run; names: Map<string, string> }) {
  const { t } = useI18n();
  const live = Object.values(run.progress);
  const current = live.find((p) => p.status === "running");
  const done = live.filter((p) => p.status !== "running").length;
  const label = current
    ? t("install.progress", {
        i: current.index + 1,
        n: current.total,
        name: names.get(current.id) ?? current.name,
      })
    : t("install.preparing");
  return (
    <div className="flex items-center gap-4">
      <Rail percent={Math.round((done / Math.max(1, run.ids.length)) * 100)} />
      <span className="num shrink-0 text-xs text-foreground/90">{label}</span>
      <Button
        variant="ghost"
        size="sm"
        disabled={run.stopping}
        onClick={() => {
          void backend.installCancel();
          // The store's copy, not this render's: progress may have moved on.
          const r = store.run;
          if (r) set({ run: { ...r, stopping: true } });
        }}
      >
        <Square className="size-3.5" />
        {run.stopping ? t("install.stopping") : t("install.stop")}
      </Button>
    </div>
  );
}

const STATUS_TONE = {
  waiting: "neutral",
  running: "accent",
  ok: "success",
  failed: "danger",
  cancelled: "neutral",
} as const;

/** Install-only reasons live in `install.reason.*`; the rest are the updates tab's texts. */
const OWN_REASONS = ["installed", "no_installer"];
const SHARED_REASONS = [
  "reboot",
  "reboot_first",
  "in_use",
  "busy",
  "cancelled",
  "admin",
  "download",
  "hash",
  "disk_full",
  "blocked",
  "start_failed",
  "timeout",
  "winget_missing",
  "not_found",
];

/** winget codes are HRESULT-like (hex); installer codes are small (decimal). */
const codeText = (c: number) =>
  c > 0xffff ? `0x${(c >>> 0).toString(16).toUpperCase().padStart(8, "0")}` : String(c);

function RunCard({
  run,
  names,
  onClose,
}: {
  run: Run;
  names: Map<string, string>;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const finished = run.result !== null;
  const reason = (p: { reason: string; code: number }) =>
    OWN_REASONS.includes(p.reason)
      ? t(`install.reason.${p.reason}` as I18nKey)
      : SHARED_REASONS.includes(p.reason)
        ? t(`updates.reason.${p.reason}` as I18nKey)
        : t("updates.reason.failed", { code: codeText(p.code) });
  const rows = run.result
    ? run.result.items.map((i) => ({ ...i, index: 0, total: run.ids.length }))
    : run.ids.map(
        (id) =>
          run.progress[id] ?? {
            id,
            name: names.get(id) ?? id,
            status: "waiting" as const,
            reason: "",
            code: 0,
            index: 0,
            total: run.ids.length,
          },
      );
  return (
    <Card className="animate-rise min-w-0">
      <CardTitle
        right={
          finished ? (
            <Button variant="ghost" size="sm" onClick={onClose}>
              {t("install.run.close")}
            </Button>
          ) : null
        }
      >
        <PackagePlus className="size-4 text-primary" />
        {t("install.run.title")}
        <Badge tone="neutral">{run.ids.length}</Badge>
      </CardTitle>
      <ul className="divide-y divide-border/70">
        {rows.map((p) => {
          const entryName = names.get(p.id) ?? p.name;
          return (
            <li
              key={p.id}
              className={cn(
                "flex items-center gap-3 py-2 transition-colors duration-200 ease-out-expo",
                p.status === "running" && "-mx-2 rounded-md bg-primary/6 px-2",
              )}
            >
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-semibold">{entryName}</span>
                <span className="block truncate font-mono text-[11px] text-muted-foreground">
                  {p.id}
                </span>
                {p.status !== "running" && p.status !== "waiting" && p.reason !== "ok" ? (
                  <span
                    className={cn(
                      "mt-0.5 block text-[11px]",
                      p.status === "failed" ? "text-destructive" : "text-muted-foreground",
                    )}
                  >
                    {reason(p)}
                  </span>
                ) : null}
              </span>
              <Badge tone={STATUS_TONE[p.status]}>
                {t(`install.status.${p.status}` as I18nKey)}
              </Badge>
            </li>
          );
        })}
      </ul>
      {run.result ? (
        <div className="mt-3 space-y-1 border-t border-border pt-3 text-xs text-foreground/90">
          <p>{t("install.run.done", counts(run.result))}</p>
          {run.result.items.some((i) => i.status !== "ok") ? (
            <p className="text-muted-foreground">{t("install.run.retry")}</p>
          ) : null}
        </div>
      ) : null}
    </Card>
  );
}

function HistoryCard({ runs }: { runs: InstallBatch[] }) {
  const { t, locale } = useI18n();
  if (runs.length === 0) return null;
  return (
    <Card className="animate-rise" style={rise(2)}>
      <CardTitle>
        <History className="size-4 text-muted-foreground" />
        {t("install.history.title")}
      </CardTitle>
      <ul className="space-y-1.5">
        {[...runs]
          .reverse()
          .slice(0, 5)
          .map((b) => {
            const c = counts(b);
            return (
              <li key={b.at} className="flex items-center gap-3 text-xs">
                <span className="num w-36 shrink-0 text-muted-foreground">
                  {new Date(b.at * 1000).toLocaleString(locale)}
                </span>
                <span className="text-foreground/90">
                  {t("install.history.row", { ok: c.ok, n: b.items.length })}
                </span>
                {c.failed ? (
                  <Badge tone="danger">
                    {c.failed} {t("install.status.failed")}
                  </Badge>
                ) : null}
                {b.reboot ? <Badge tone="warning">{t("updates.reason.reboot")}</Badge> : null}
                <HelpHint
                  text={b.items
                    .map((i) => `${i.name}: ${t(`install.status.${i.status}` as I18nKey)}`)
                    .join("\n")}
                />
              </li>
            );
          })}
      </ul>
    </Card>
  );
}
