import {
  Ban,
  Cpu,
  Download,
  History,
  PackageCheck,
  Power,
  RefreshCw,
  ShieldAlert,
  Square,
  Undo2,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  HelpHint,
  Modal,
  Skeleton,
} from "@/components/ui/primitives";
import {
  type AppUpdate,
  type AppUpdateProgress,
  backend,
  inTauri,
  type UpdateBatch,
  type UpdateItem,
  type WuProgress,
  type WuScan,
  type WuStatus,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

/* ------------------------------------------------------------------------------------------------
 * Module store: a batch or a Windows install runs for minutes, so its progress must survive the
 * user leaving the tab (same idea as the console blocks).
 * ---------------------------------------------------------------------------------------------- */

type AppsRun = {
  ids: string[];
  progress: Record<string, AppUpdateProgress>;
  result: UpdateBatch | null;
  stopping: boolean;
};
type Store = {
  apps: AppUpdate[] | null;
  appsBusy: boolean;
  appsError: string | null;
  appsRun: AppsRun | null;
  wu: WuScan | null;
  wuBusy: "search" | "install" | null;
  wuError: string | null;
  wuProgress: WuProgress | null;
  wuResult: UpdateBatch | null;
  status: WuStatus | null;
  runs: UpdateBatch[];
};
let store: Store = {
  apps: null,
  appsBusy: false,
  appsError: null,
  appsRun: null,
  wu: null,
  wuBusy: null,
  wuError: null,
  wuProgress: null,
  wuResult: null,
  status: null,
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

async function loadApps() {
  set({ appsBusy: true, appsError: null, appsRun: null });
  try {
    set({ apps: await backend.updatesApps() });
  } catch (e) {
    set({ appsError: errCode(e) });
  }
  set({ appsBusy: false });
}

async function loadRuns() {
  set({ runs: (await backend.updatesHistory()) ?? [] });
}

async function runApps(ids: string[]) {
  set({ appsRun: { ids, progress: {}, result: null, stopping: false }, appsError: null });
  const off = await backend.on<AppUpdateProgress>("updates:apps", (p) => {
    const r = store.appsRun;
    if (r) set({ appsRun: { ...r, progress: { ...r.progress, [p.id]: p } } });
  });
  try {
    const token = await backend.updatesAppsConfirm(ids);
    const result = await backend.updatesAppsRun(token.token, ids);
    const r = store.appsRun;
    if (r) set({ appsRun: { ...r, result } });
    void loadRuns();
  } catch (e) {
    set({ appsError: errCode(e), appsRun: null });
  } finally {
    off();
  }
}

async function loadStatus() {
  const s = await backend.updatesWuStatus();
  if (s) set({ status: s });
}

async function searchWu(online: boolean) {
  set({ wuBusy: "search", wuError: null, wuResult: null });
  try {
    set({ wu: await backend.updatesWuSearch(online) });
  } catch (e) {
    set({ wuError: errCode(e) });
  }
  set({ wuBusy: null });
}

async function installWu(ids: string[]) {
  set({ wuBusy: "install", wuError: null, wuProgress: null, wuResult: null });
  const off = await backend.on<WuProgress>("updates:wu", (p) => set({ wuProgress: p }));
  try {
    const token = await backend.updatesWuConfirm(ids);
    set({ wuResult: await backend.updatesWuInstall(token.token, ids) });
    void loadStatus();
    void loadRuns();
  } catch (e) {
    set({ wuError: errCode(e) });
  } finally {
    off();
    set({ wuBusy: null, wuProgress: null });
  }
}

/* ------------------------------------------------------------------------------------------------ */

const REASONS = [
  "ok",
  "reboot",
  "reboot_first",
  "in_use",
  "busy",
  "cancelled",
  "admin",
  "not_applicable",
  "download",
  "hash",
  "disk_full",
  "blocked",
  "start_failed",
  "timeout",
  "winget_missing",
  "partial",
  "not_found",
] as const;
const STATUS_TONE = {
  waiting: "neutral",
  running: "accent",
  ok: "success",
  failed: "danger",
  cancelled: "neutral",
} as const;
const SEV_TONE: Record<string, "danger" | "warning" | "neutral"> = {
  Critical: "danger",
  Important: "warning",
};
const RESULT_TONE = {
  ok: "success",
  partial: "warning",
  failed: "danger",
  cancelled: "neutral",
  in_progress: "accent",
} as const;

/** winget codes are HRESULT-like (hex); installer codes are small (decimal). */
const codeText = (c: number) =>
  c > 0xffff ? `0x${(c >>> 0).toString(16).toUpperCase().padStart(8, "0")}` : String(c);

function useTexts() {
  const { t } = useI18n();
  const fallback = useErrorText();
  const error = useCallback(
    (code: string) => {
      if (code === "busy") return t("updates.error.busy");
      if (code === "timeout") return t("updates.error.timeout");
      if (code === "bad_selection" || code === "invalid_id")
        return t("updates.error.bad_selection");
      if (code === "no_reboot_needed") return t("updates.error.no_reboot_needed");
      if (code === "winget_unavailable") return t("updates.error.winget_missing");
      if (code.startsWith("wu_0x")) return t("updates.error.wu", { code: code.slice(3) });
      return fallback(code);
    },
    [t, fallback],
  );
  const reason = useCallback(
    (i: UpdateItem) =>
      (REASONS as readonly string[]).includes(i.reason)
        ? t(`updates.reason.${i.reason}` as I18nKey)
        : t("updates.reason.failed", { code: codeText(i.code) }),
    [t],
  );
  return { error, reason };
}

const counts = (b: UpdateBatch) => ({
  ok: b.items.filter((i) => i.status === "ok").length,
  failed: b.items.filter((i) => i.status === "failed").length,
  cancelled: b.items.filter((i) => i.status === "cancelled").length,
});

/** Thin rail: determinate when a percentage is known, a sweep otherwise. */
function Rail({ percent }: { percent: number | null }) {
  return (
    <span
      className="relative block h-1 flex-1 overflow-hidden rounded-full bg-border"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={percent ?? undefined}
    >
      {percent !== null ? (
        <span
          className="block h-full rounded-full bg-primary transition-[width] duration-300"
          style={{ width: `${percent}%` }}
        />
      ) : (
        <span className="absolute inset-y-0 w-1/3 animate-sweep rounded-full bg-primary/70" />
      )}
    </span>
  );
}

export function UpdatesTab() {
  const [elevated, setElevated] = useState(false);
  useEffect(() => {
    backend.appInfo().then((i) => i && setElevated(i.elevated));
    if (!inTauri) return;
    if (store.apps === null && !store.appsBusy && !store.appsRun) void loadApps();
    if (store.wu === null && store.wuBusy === null) void searchWu(false);
    void loadStatus();
    void loadRuns();
  }, []);
  return (
    <div className="space-y-4">
      <AppsSection elevated={elevated} />
      <WindowsSection elevated={elevated} />
      <RunsCard />
    </div>
  );
}

/* ---------------- apps ---------------- */

function AppsSection({ elevated }: { elevated: boolean }) {
  const { t } = useI18n();
  const { error, reason } = useTexts();
  const s = useStore();
  // Ticks the user removed: everything "update all" would take starts ticked.
  const [unpicked, setUnpicked] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const run = s.appsRun;
  const running = run !== null && run.result === null;

  const eligible = (s.apps ?? []).filter((a) => a.skip === null);
  const chosen = eligible.filter((a) => !unpicked.has(a.id));
  const machine = chosen.filter((a) => a.machine).length;
  const current = run ? Object.values(run.progress).find((p) => p.status === "running") : undefined;
  const done = run ? Object.values(run.progress).filter((p) => p.status !== "running").length : 0;

  const toggle = (id: string) =>
    setUnpicked((cur) => {
      const n = new Set(cur);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  const exclude = async (a: AppUpdate) => {
    const on = a.skip !== "excluded";
    try {
      await backend.updatesExclude(a.id, on);
      set({
        apps: (store.apps ?? []).map((x) =>
          x.id === a.id ? { ...x, skip: on ? "excluded" : null } : x,
        ),
      });
    } catch (e) {
      setMsg(error(errCode(e)));
    }
  };

  const start = () => {
    setConfirm(false);
    setMsg(null);
    // Order of the list on screen: the token is bound to this exact sequence.
    void runApps(chosen.map((a) => a.id));
  };

  return (
    <Card className="animate-rise min-w-0">
      <CardTitle
        help={t("updates.apps.help")}
        right={
          <span className="flex flex-wrap items-center justify-end gap-2">
            {running ? (
              <Button
                variant="ghost"
                size="sm"
                disabled={run.stopping}
                onClick={() => {
                  void backend.updatesAppsCancel();
                  // The store's copy, not this render's: progress may have moved on.
                  const r = store.appsRun;
                  if (r) set({ appsRun: { ...r, stopping: true } });
                }}
              >
                <Square className="size-3.5" />
                {run.stopping ? t("updates.apps.stopping") : t("updates.apps.stop")}
              </Button>
            ) : (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  disabled={!inTauri || s.appsBusy}
                  onClick={() => void loadApps()}
                >
                  <RefreshCw className={cn("size-4", s.appsBusy && "animate-spin")} />
                  {t("updates.apps.search")}
                </Button>
                <Button
                  variant="primary"
                  size="sm"
                  disabled={!inTauri || chosen.length === 0 || s.appsBusy || run !== null}
                  onClick={() => setConfirm(true)}
                >
                  <Download className="size-4" />
                  {t("updates.apps.update", { n: chosen.length })}
                </Button>
              </>
            )}
          </span>
        }
      >
        <PackageCheck className="size-4 text-primary" />
        {t("updates.apps.title")}
        {s.apps ? (
          <Badge tone="neutral">{t("updates.apps.count", { n: s.apps.length })}</Badge>
        ) : null}
      </CardTitle>

      {!elevated && machine > 0 && !run ? (
        <p className="-mt-1 mb-3 flex items-start gap-2 rounded-md bg-warning/8 px-3 py-2 text-xs text-foreground/90 ring-1 ring-warning/25 ring-inset">
          <ShieldAlert className="mt-0.5 size-3.5 shrink-0 text-warning" />
          {t("updates.apps.uac")}
        </p>
      ) : null}

      {running ? (
        <div className="mb-3 flex items-center gap-3">
          <Rail percent={Math.round((done / Math.max(1, run.ids.length)) * 100)} />
          <span className="num shrink-0 text-xs text-muted-foreground">
            {current
              ? t("updates.apps.progress", {
                  i: current.index + 1,
                  n: current.total,
                  name: current.name,
                })
              : t("updates.wu.preparing")}
          </span>
        </div>
      ) : null}

      {s.appsBusy ? (
        <div className="space-y-2">
          <p className="text-xs text-muted-foreground">{t("updates.apps.searching")}</p>
          {[0, 1, 2].map((k) => (
            <Skeleton key={k} className="h-11 w-full" />
          ))}
        </div>
      ) : null}
      {s.apps && s.apps.length === 0 && !s.appsBusy ? (
        <p className="py-6 text-center text-xs text-muted-foreground">{t("updates.apps.none")}</p>
      ) : null}

      {s.apps && s.apps.length > 0 && !s.appsBusy ? (
        <>
          {!run ? (
            <div className="mb-1 flex gap-1 text-xs">
              <button
                type="button"
                className="press rounded px-1.5 py-0.5 text-muted-foreground hover:text-foreground"
                onClick={() => setUnpicked(new Set())}
              >
                {t("updates.apps.select_all")}
              </button>
              <button
                type="button"
                className="press rounded px-1.5 py-0.5 text-muted-foreground hover:text-foreground"
                onClick={() => setUnpicked(new Set(eligible.map((a) => a.id)))}
              >
                {t("updates.apps.select_none")}
              </button>
            </div>
          ) : null}
          <ul className="divide-y divide-border/70">
            {s.apps.map((a) => {
              const p = run?.progress[a.id];
              const queued = run?.ids.includes(a.id) ?? false;
              const status = p?.status ?? (queued ? "waiting" : null);
              return (
                <li
                  key={a.id}
                  className={cn(
                    "flex items-center gap-3 py-2.5 transition-colors duration-200 ease-out-expo",
                    status === "running" && "-mx-2 rounded-md bg-primary/6 px-2",
                    a.skip !== null && "opacity-70",
                  )}
                >
                  <input
                    type="checkbox"
                    checked={a.skip === null && !unpicked.has(a.id)}
                    disabled={a.skip !== null || run !== null}
                    onChange={() => toggle(a.id)}
                    aria-label={a.name}
                    className="size-4 accent-primary"
                  />
                  <div className="min-w-0 flex-1">
                    <p className="flex flex-wrap items-center gap-1.5">
                      <span className="truncate text-sm font-semibold">{a.name}</span>
                      {a.machine ? (
                        <span title={t("updates.apps.machine.hint")}>
                          <Badge tone="neutral">{t("updates.apps.machine")}</Badge>
                        </span>
                      ) : null}
                      {a.skip ? (
                        <Badge tone={a.skip === "excluded" ? "warning" : "neutral"}>
                          {t(`updates.apps.skip.${a.skip}` as I18nKey)}
                        </Badge>
                      ) : null}
                    </p>
                    <p className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
                      {a.id}
                      <span className="num ml-2 text-foreground/80">
                        {a.version} → {a.available}
                      </span>
                    </p>
                    {p && p.status !== "running" && p.reason !== "ok" ? (
                      <p
                        className={cn(
                          "mt-0.5 text-[11px]",
                          p.status === "failed" ? "text-destructive" : "text-muted-foreground",
                        )}
                      >
                        {reason(p)}
                      </p>
                    ) : null}
                  </div>
                  {status ? (
                    <Badge tone={STATUS_TONE[status]}>
                      {t(`updates.status.${status}` as I18nKey)}
                    </Badge>
                  ) : null}
                  {!run && a.skip !== "self" && a.skip !== "unknown_version" ? (
                    <Button
                      variant="ghost"
                      size="sm"
                      onClick={() => void exclude(a)}
                      title={
                        a.skip === "excluded"
                          ? t("updates.apps.include")
                          : t("updates.apps.exclude")
                      }
                    >
                      {a.skip === "excluded" ? (
                        <Undo2 className="size-3.5" />
                      ) : (
                        <Ban className="size-3.5" />
                      )}
                      <span className="hidden md:inline">
                        {a.skip === "excluded"
                          ? t("updates.apps.include")
                          : t("updates.apps.exclude")}
                      </span>
                    </Button>
                  ) : null}
                </li>
              );
            })}
          </ul>
        </>
      ) : null}

      {run?.result ? (
        <p className="mt-3 border-t border-border pt-3 text-xs text-foreground/90">
          {t("updates.apps.done", counts(run.result))}
        </p>
      ) : null}
      {s.appsError ? <p className="mt-2 text-xs text-destructive">{error(s.appsError)}</p> : null}
      {msg ? <p className="mt-2 text-xs text-destructive">{msg}</p> : null}

      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        closeLabel={t("common.cancel")}
        title={t("updates.apps.confirm.title", { n: chosen.length })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" onClick={start}>
              {t("updates.apps.confirm.go")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("updates.apps.confirm.body")}</p>
        {machine > 0 && !elevated ? (
          <p className="mt-3 rounded-md bg-warning/10 px-3 py-2 text-warning ring-1 ring-warning/25 ring-inset">
            {t("updates.apps.confirm.uac", { n: machine })}
          </p>
        ) : null}
        <ul className="mt-3 max-h-48 list-disc space-y-0.5 overflow-y-auto pl-5 text-muted-foreground">
          {chosen.map((a) => (
            <li key={a.id}>
              {a.name} <span className="num text-foreground/80">→ {a.available}</span>
            </li>
          ))}
        </ul>
      </Modal>
    </Card>
  );
}

/* ---------------- Windows Update ---------------- */

function WindowsSection({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const { error, reason } = useTexts();
  const s = useStore();
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [confirm, setConfirm] = useState(false);
  const [reboot, setReboot] = useState(false);
  const [rebootErr, setRebootErr] = useState<string | null>(null);
  const [kbOnly, setKbOnly] = useState(true);
  const list = s.wu?.updates ?? [];

  // Optional drivers stay unticked, as in Settings.
  useEffect(() => {
    setPicked(new Set((s.wu?.updates ?? []).filter((u) => !u.driver).map((u) => u.id)));
  }, [s.wu]);

  const chosen = list.filter((u) => picked.has(u.id));
  const results = useMemo(
    () => Object.fromEntries((s.wuResult?.items ?? []).map((i) => [i.id, i])),
    [s.wuResult],
  );
  const pending = !!(s.status?.reboot_required || s.wu?.reboot_required || s.wuResult?.reboot);
  const busy = s.wuBusy !== null;
  const history = (s.status?.history ?? []).filter((h) => !kbOnly || h.kb);
  const p = s.wuProgress;

  const toggle = (id: string) =>
    setPicked((cur) => {
      const n = new Set(cur);
      if (n.has(id)) n.delete(id);
      else n.add(id);
      return n;
    });

  return (
    <>
      {pending ? (
        <Card className="animate-rise flex flex-wrap items-center gap-4 border-warning/40 py-4">
          <Power className="size-5 text-warning" />
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="font-display text-[15px] font-semibold tracking-wide">
              {t("updates.wu.reboot.title")}
            </span>
            <span className="text-xs text-muted-foreground">{t("updates.wu.reboot.body")}</span>
            {rebootErr ? <span className="text-xs text-destructive">{rebootErr}</span> : null}
          </span>
          <Button variant="outline" onClick={() => setReboot(true)}>
            <Power className="size-4" />
            {t("updates.wu.reboot.now")}
          </Button>
        </Card>
      ) : null}

      {!elevated ? (
        <Card
          className="animate-rise flex flex-wrap items-center gap-4 border-warning/40 py-4"
          style={rise(1)}
        >
          <span className="flex min-w-0 flex-1 flex-col gap-0.5">
            <span className="font-display text-[15px] font-semibold tracking-wide">
              {t("updates.wu.admin.title")}
            </span>
            <span className="text-xs text-muted-foreground">{t("updates.wu.admin.body")}</span>
          </span>
          <ElevateButton route="apps" size="md" />
        </Card>
      ) : null}

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[1fr_minmax(320px,420px)]">
        <Card className="animate-rise min-w-0" style={rise(2)}>
          <CardTitle
            help={t("updates.wu.help")}
            right={
              <span className="flex flex-wrap items-center justify-end gap-2">
                {busy ? (
                  <Button variant="ghost" size="sm" onClick={() => void backend.updatesWuCancel()}>
                    <Square className="size-3.5" />
                    {t("common.stop")}
                  </Button>
                ) : (
                  <>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={!inTauri}
                      onClick={() => void searchWu(true)}
                    >
                      <RefreshCw className="size-4" />
                      {t("updates.wu.check")}
                    </Button>
                    <Button
                      variant="primary"
                      size="sm"
                      disabled={!inTauri || !elevated || chosen.length === 0}
                      title={!elevated ? t("common.admin_required") : undefined}
                      onClick={() => setConfirm(true)}
                    >
                      <Download className="size-4" />
                      {t("updates.wu.install", { n: chosen.length })}
                    </Button>
                  </>
                )}
              </span>
            }
          >
            {t("updates.wu.title")}
            {s.wu ? (
              <Badge tone={list.length ? "accent" : "neutral"}>
                {t("updates.wu.count", { n: list.length })}
              </Badge>
            ) : null}
          </CardTitle>

          {s.wuBusy === "search" ? (
            <div className="mb-3 flex items-center gap-3">
              <Rail percent={null} />
              <span className="shrink-0 text-xs text-muted-foreground">
                {t("updates.wu.checking")}
              </span>
            </div>
          ) : s.wu ? (
            <p className="-mt-2 mb-3 text-[11px] text-muted-foreground">
              {s.wu.online ? t("updates.wu.online") : t("updates.wu.cached")}
            </p>
          ) : null}

          {s.wuBusy === "install" ? (
            <div className="mb-3 flex items-center gap-3">
              <Rail percent={p ? p.percent : null} />
              <span className="num shrink-0 text-xs text-muted-foreground">
                {!p
                  ? t("updates.wu.preparing")
                  : p.phase === "download"
                    ? t("updates.wu.phase.download", { p: p.percent })
                    : t("updates.wu.phase.install", { i: p.current + 1, n: p.total, p: p.percent })}
              </span>
            </div>
          ) : null}

          {s.wu === null && s.wuBusy === "search"
            ? [0, 1].map((k) => <Skeleton key={k} className="my-2 h-12 w-full" />)
            : null}
          {s.wu && list.length === 0 && s.wuBusy !== "search" ? (
            <p className="py-6 text-center text-xs text-muted-foreground">{t("updates.wu.none")}</p>
          ) : null}

          <ul className="divide-y divide-border/70">
            {list.map((u) => {
              const r = results[u.id];
              return (
                <li key={u.id} className="flex items-start gap-3 py-2.5">
                  <input
                    type="checkbox"
                    checked={picked.has(u.id)}
                    disabled={busy}
                    onChange={() => toggle(u.id)}
                    aria-label={u.title}
                    className="mt-1 size-4 accent-primary"
                  />
                  {u.driver ? (
                    <Cpu className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                  ) : (
                    <Download className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                  )}
                  <div className="min-w-0 flex-1">
                    <p className="text-[13px] leading-snug">{u.title}</p>
                    <p className="mt-1 flex flex-wrap items-center gap-1.5 text-[11px] text-muted-foreground">
                      {u.kb.map((k) => (
                        <span key={k} className="num font-mono">
                          {k}
                        </span>
                      ))}
                      {u.severity ? (
                        <Badge tone={SEV_TONE[u.severity] ?? "neutral"}>
                          {["Critical", "Important", "Moderate", "Low"].includes(u.severity)
                            ? t(`updates.wu.sev.${u.severity}` as I18nKey)
                            : u.severity}
                        </Badge>
                      ) : null}
                      {u.driver ? <Badge tone="neutral">{t("updates.wu.driver")}</Badge> : null}
                      {u.mandatory ? (
                        <Badge tone="neutral">{t("updates.wu.mandatory")}</Badge>
                      ) : null}
                      {u.reboot !== "never" ? (
                        <Badge tone="neutral">
                          {t(`updates.wu.reboot.${u.reboot}` as I18nKey)}
                        </Badge>
                      ) : null}
                      {u.downloaded ? (
                        <Badge tone="neutral">{t("updates.wu.downloaded")}</Badge>
                      ) : null}
                      {u.categories.slice(0, 2).map((c) => (
                        <span key={c}>{c}</span>
                      ))}
                    </p>
                    {r ? (
                      <p
                        className={cn(
                          "mt-1 flex items-center gap-1.5 text-[11px]",
                          r.status === "failed" ? "text-destructive" : "text-foreground/85",
                        )}
                      >
                        <Badge tone={STATUS_TONE[r.status]}>
                          {t(`updates.status.${r.status}` as I18nKey)}
                        </Badge>
                        {r.reason !== "ok" ? reason(r) : null}
                      </p>
                    ) : null}
                  </div>
                  <span className="num shrink-0 text-xs text-muted-foreground">
                    {u.size ? formatBytes(u.size, locale) : ""}
                  </span>
                </li>
              );
            })}
          </ul>

          {s.wuResult ? (
            <p className="mt-3 border-t border-border pt-3 text-xs text-foreground/90">
              {t("updates.wu.done", counts(s.wuResult))}
            </p>
          ) : null}
          {s.wuError ? <p className="mt-2 text-xs text-destructive">{error(s.wuError)}</p> : null}
        </Card>

        <Card className="animate-rise min-w-0" style={rise(3)}>
          <CardTitle
            help={t("updates.wu.history.help")}
            right={
              <label className="flex items-center gap-1.5 text-xs text-muted-foreground">
                <input
                  type="checkbox"
                  checked={kbOnly}
                  onChange={() => setKbOnly((v) => !v)}
                  className="size-3.5 accent-primary"
                />
                {t("updates.wu.history.kb_only")}
              </label>
            }
          >
            <History className="size-4 text-muted-foreground" />
            {t("updates.wu.history.title")}
          </CardTitle>
          {s.status === null && inTauri
            ? [0, 1, 2].map((k) => <Skeleton key={k} className="my-2 h-9 w-full" />)
            : null}
          {s.status && history.length === 0 ? (
            <p className="py-6 text-center text-xs text-muted-foreground">
              {t("updates.wu.history.none")}
            </p>
          ) : null}
          <ul className="max-h-120 space-y-2 overflow-y-auto pr-1">
            {history.map((h) => (
              <li key={`${h.at}:${h.title}`} className="flex items-start gap-2">
                <span className="num w-20 shrink-0 pt-0.5 text-[11px] text-muted-foreground">
                  {new Date(h.at * 1000).toLocaleDateString(locale)}
                </span>
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-xs" title={h.title}>
                    {h.title}
                  </span>
                  <span className="mt-0.5 flex items-center gap-1.5">
                    {h.kb ? <span className="num font-mono text-[11px]">{h.kb}</span> : null}
                    <Badge tone={RESULT_TONE[h.result]}>
                      {h.operation === "uninstall"
                        ? t("updates.wu.history.uninstall")
                        : t(`updates.wu.result.${h.result}` as I18nKey)}
                    </Badge>
                  </span>
                </span>
              </li>
            ))}
          </ul>
        </Card>
      </div>

      <Modal
        open={confirm}
        onClose={() => setConfirm(false)}
        closeLabel={t("common.cancel")}
        title={t("updates.wu.confirm.title", { n: chosen.length })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              onClick={() => {
                setConfirm(false);
                void installWu(chosen.map((u) => u.id));
              }}
            >
              {t("updates.wu.confirm.go")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("updates.wu.confirm.body")}</p>
        {chosen.some((u) => u.driver) ? (
          <p className="mt-3 rounded-md bg-warning/10 px-3 py-2 text-warning ring-1 ring-warning/25 ring-inset">
            {t("updates.wu.confirm.driver")}
          </p>
        ) : null}
        <ul className="mt-3 max-h-48 list-disc space-y-0.5 overflow-y-auto pl-5 text-muted-foreground">
          {chosen.map((u) => (
            <li key={u.id}>{u.title}</li>
          ))}
        </ul>
      </Modal>

      <Modal
        open={reboot}
        onClose={() => setReboot(false)}
        closeLabel={t("common.cancel")}
        title={t("updates.wu.reboot.confirm.title")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setReboot(false)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                setReboot(false);
                setRebootErr(null);
                backend.updatesReboot().catch((e) => setRebootErr(error(errCode(e))));
              }}
            >
              <Power className="size-4" />
              {t("updates.wu.reboot.now")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("updates.wu.reboot.confirm.body")}</p>
      </Modal>
    </>
  );
}

/* ---------------- G2 Boost's own runs ---------------- */

function RunsCard() {
  const { t, locale } = useI18n();
  const { runs } = useStore();
  if (runs.length === 0) return null;
  return (
    <Card className="animate-rise" style={rise(4)}>
      <CardTitle>
        <History className="size-4 text-muted-foreground" />
        {t("updates.runs.title")}
      </CardTitle>
      <ul className="space-y-1.5">
        {[...runs]
          .reverse()
          .slice(0, 6)
          .map((b) => {
            const c = counts(b);
            return (
              <li key={`${b.at}:${b.kind}`} className="flex items-center gap-3 text-xs">
                <span className="num w-36 shrink-0 text-muted-foreground">
                  {new Date(b.at * 1000).toLocaleString(locale)}
                </span>
                <Badge tone="neutral">{t(`updates.runs.kind.${b.kind}` as I18nKey)}</Badge>
                <span className="text-foreground/90">
                  {t("updates.runs.row", { ok: c.ok, n: b.items.length })}
                </span>
                {c.failed ? (
                  <Badge tone="danger">
                    {c.failed} {t("updates.status.failed")}
                  </Badge>
                ) : null}
                {b.reboot ? <Badge tone="warning">{t("updates.reason.reboot")}</Badge> : null}
                <HelpHint
                  text={b.items
                    .map((i) => `${i.name}: ${t(`updates.status.${i.status}` as I18nKey)}`)
                    .join("\n")}
                />
              </li>
            );
          })}
      </ul>
    </Card>
  );
}
