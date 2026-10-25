import { History, RotateCcw, Undo2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import type { Route } from "@/components/shell/app-shell";
import { rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  Meter,
  Modal,
  Skeleton,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import {
  backend,
  type HistoryEntry,
  type ProfilePlan,
  type Restoration,
  type RestoreKind,
} from "@/lib/backend";
import { useLatest } from "@/lib/boost-hooks";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";
import { tweakLabel } from "./shared";

/** Where each saved change is undone. The route is the screen's; the tab is named in the text. */
const WHERE: Record<RestoreKind, { route: Route; label: I18nKey }> = {
  dns: { route: "boost", label: "nav.boost" },
  gpu_driver: { route: "gpu", label: "gpu.nav" },
  gpu_pref: { route: "games", label: "nav.games" },
  exposure: { route: "security", label: "security.nav" },
  qos: { route: "games", label: "nav.games" },
  apps_undo: { route: "apps", label: "nav.apps" },
  game_mode: { route: "games", label: "nav.games" },
  peripherals: { route: "peripherals", label: "nav.peripherals" },
};

const CHIP = "normal-case tracking-normal text-[11px]";

/** The same tweak flipped twice in one second still gets two distinct keys. */
function historyRows(list: HistoryEntry[]) {
  const seen = new Map<string, number>();
  return list.map((h) => {
    const base = `${h.at}-${h.id}-${h.on}`;
    const n = (seen.get(base) ?? 0) + 1;
    seen.set(base, n);
    return { h, key: `${base}-${n}` };
  });
}

export function RestoreTab({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  // undefined = loading; null = nothing answered (outside the app).
  const [data, setData] = useState<Restoration | null | undefined>(undefined);
  const [history, setHistory] = useState<HistoryEntry[] | null | undefined>(undefined);
  const [busy, setBusy] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [all, setAll] = useState<{
    plan: ProfilePlan | null;
    run: { done: number; total: number } | null;
  } | null>(null);

  const load = useCallback(async () => {
    const [r, h] = await Promise.all([backend.profilesRestoration(), backend.profilesHistory()]);
    setData(r);
    setHistory(h);
  }, []);
  useEffect(() => {
    load();
    const onBack = () => document.visibilityState === "visible" && load();
    window.addEventListener("focus", onBack);
    document.addEventListener("visibilitychange", onBack);
    return () => {
      window.removeEventListener("focus", onBack);
      document.removeEventListener("visibilitychange", onBack);
    };
  }, [load]);

  const undo = async (id: string) => {
    setBusy(id);
    setErrors((m) => ({ ...m, [id]: "" }));
    try {
      const r = await backend.profilesStep({ id, op: "off", level: null }, "@undo");
      if (r.ok) toast(t("profiles.restore.done", { name: tweakLabel(t, id) }), { tone: "success" });
      else setErrors((m) => ({ ...m, [id]: errorText(new Error(r.error ?? "")) }));
    } catch (e) {
      setErrors((m) => ({ ...m, [id]: errorText(e) }));
    } finally {
      setBusy(null);
      await load();
    }
  };

  const openAll = async () => {
    setAll({ plan: null, run: null });
    try {
      const plan = await backend.profilesPlan([], null, true);
      setAll((a) => (a ? { ...a, plan } : a));
    } catch (e) {
      setAll(null);
      toast(errorText(e), { tone: "warning" });
    }
  };

  const undoAll = async () => {
    const steps = all?.plan?.steps ?? [];
    if (steps.length === 0) return;
    let ok = 0;
    setAll((a) => (a ? { ...a, run: { done: 0, total: steps.length } } : a));
    for (const [i, step] of steps.entries()) {
      try {
        if ((await backend.profilesStep(step, "@undo_all")).ok) ok += 1;
      } catch {
        /* the next step still runs; the history keeps what failed */
      }
      setAll((a) => (a ? { ...a, run: { done: i + 1, total: steps.length } } : a));
    }
    setAll(null);
    toast(t("profiles.restore.all_done", { ok, total: steps.length }), {
      tone: ok === steps.length ? "success" : "warning",
    });
    await load();
  };

  const shownAll = useLatest(all); // the dialog keeps its content while it fades out
  const noBackupAll = (shownAll?.plan?.steps ?? []).filter(
    (st) => !data?.tweaks.find((a) => a.id === st.id)?.has_backup,
  ).length;
  const stamp = new Intl.DateTimeFormat(locale, { dateStyle: "short", timeStyle: "short" });
  const locked = (data?.tweaks ?? []).some((a) => a.admin) && !elevated;
  const count = data?.tweaks.length ?? 0;

  return (
    <div className="grid grid-cols-1 gap-4 xl:grid-cols-5">
      <Card className="animate-rise xl:col-span-3">
        <CardTitle
          help={t("profiles.restore.help")}
          right={
            <div className="flex items-center gap-2">
              {locked ? <ElevateButton route="profiles" tab="restore" /> : null}
              <Button
                variant="danger"
                size="sm"
                disabled={count === 0 || busy !== null}
                onClick={openAll}
              >
                <RotateCcw className="size-4" />
                {t("profiles.restore.undo_all")}
              </Button>
            </div>
          }
        >
          {t("profiles.restore.title")}
          {count > 0 ? (
            <Badge tone="neutral" className="num">
              {t("profiles.restore.count", { count })}
            </Badge>
          ) : null}
        </CardTitle>
        {data === undefined ? (
          <div className="flex flex-col gap-3">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </div>
        ) : !data || data.tweaks.length === 0 ? (
          <EmptyState
            icon={<Undo2 className="size-5" />}
            title={t("profiles.restore.empty.title")}
            body={t("profiles.restore.empty.body")}
          />
        ) : (
          <ul className="divide-y divide-border">
            {data.tweaks.map((a) => (
              <li key={a.id} className="flex items-center gap-3 py-2.5" data-tweak={a.id}>
                <span
                  aria-hidden
                  className={cn(
                    "size-2 shrink-0 rounded-full",
                    a.status === "on"
                      ? "bg-primary shadow-[0_0_6px_hsl(var(--primary)/0.6)]"
                      : "bg-warning",
                  )}
                />
                <div className="min-w-0 flex-1">
                  <p className="truncate text-[13px] font-semibold">{tweakLabel(t, a.id)}</p>
                  <div className="mt-1 flex flex-wrap items-center gap-1.5">
                    {a.has_backup ? (
                      <Badge tone="success" title={t("profiles.restore.backup.hint")}>
                        {t("profiles.restore.backup")}
                      </Badge>
                    ) : (
                      <Badge tone="neutral" title={t("profiles.restore.nobackup.hint")}>
                        {t("profiles.restore.nobackup")}
                      </Badge>
                    )}
                    {a.status === "partial" ? (
                      <Badge tone="warning">{t("profiles.restore.partial")}</Badge>
                    ) : null}
                    {a.admin ? <Badge tone="neutral">{t("common.admin_required")}</Badge> : null}
                  </div>
                  {a.detail === "reboot_pending" ? (
                    <p className="mt-1 text-[11px] text-warning">
                      {t("boost.state.reboot_pending")}
                    </p>
                  ) : null}
                  {errors[a.id] ? (
                    <p className="mt-1 text-xs text-destructive">{errors[a.id]}</p>
                  ) : null}
                </div>
                <Button
                  size="sm"
                  variant="outline"
                  disabled={busy !== null || (a.admin && !elevated)}
                  onClick={() => undo(a.id)}
                >
                  {t("profiles.restore.undo")}
                </Button>
              </li>
            ))}
          </ul>
        )}
      </Card>

      <div className="flex flex-col gap-4 xl:col-span-2">
        <Card className="animate-rise" style={rise(1)}>
          <CardTitle help={t("profiles.others.help")}>{t("profiles.others.title")}</CardTitle>
          {data === undefined ? (
            <Skeleton className="h-10 w-full" />
          ) : !data || data.others.length === 0 ? (
            <p className="text-xs text-muted-foreground">{t("profiles.others.empty")}</p>
          ) : (
            <ul className="divide-y divide-border">
              {data.others.map((o) => {
                const where = WHERE[o.kind];
                if (!where) return null;
                const names = o.names.map((n) =>
                  n === "global" ? t("profiles.others.global") : n,
                );
                return (
                  <li key={o.kind} className="flex items-center gap-3 py-2.5" data-other={o.kind}>
                    <div className="min-w-0 flex-1">
                      <p className="text-[13px] font-semibold">
                        {t(`profiles.others.${o.kind}` as I18nKey)}{" "}
                        <span className="num font-normal text-muted-foreground">({o.count})</span>
                      </p>
                      {names.length > 0 ? (
                        <p className="truncate text-xs text-muted-foreground">{names.join(", ")}</p>
                      ) : null}
                      <p className="text-[11px] text-muted-foreground">
                        {t("profiles.others.in", { where: t(where.label) })}
                      </p>
                    </div>
                    <Button size="sm" variant="outline" onClick={() => navigate(where.route)}>
                      {t("profiles.others.open")}
                    </Button>
                  </li>
                );
              })}
            </ul>
          )}
        </Card>

        <Card className="animate-rise" style={rise(2)}>
          <CardTitle help={t("profiles.history.help")}>
            <History className="size-4" />
            {t("profiles.history.title")}
          </CardTitle>
          {history === undefined ? (
            <Skeleton className="h-16 w-full" />
          ) : !history || history.length === 0 ? (
            <p className="text-xs text-muted-foreground">{t("profiles.history.empty")}</p>
          ) : (
            <ul className="flex max-h-90 flex-col overflow-auto pr-1">
              {historyRows(history).map(({ h, key }) => {
                const periph = h.id.startsWith("periph_");
                const action = !h.ok
                  ? "failed"
                  : h.on
                    ? periph
                      ? "set"
                      : "on"
                    : periph
                      ? "restored"
                      : "off";
                const source = h.source.startsWith("@")
                  ? (`profiles.source.${h.source.slice(1)}` as I18nKey)
                  : null;
                return (
                  <li
                    key={key}
                    className="flex items-start gap-3 border-b border-border py-2 last:border-0"
                  >
                    <time
                      dateTime={new Date(h.at * 1000).toISOString()}
                      className="num w-24 shrink-0 pt-0.5 text-[11px] leading-tight text-muted-foreground"
                    >
                      {stamp.format(new Date(h.at * 1000))}
                    </time>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-xs font-semibold">
                        {tweakLabel(t, h.id)}
                      </span>
                      <span className="block truncate text-[11px] text-muted-foreground">
                        {source ? t(source) : h.source}
                      </span>
                    </span>
                    <Badge
                      tone={
                        action === "failed"
                          ? "danger"
                          : action === "off" || action === "restored"
                            ? "neutral"
                            : "accent"
                      }
                      className={CHIP}
                      title={h.error ?? undefined}
                    >
                      {t(`profiles.history.${action}` as I18nKey)}
                    </Badge>
                  </li>
                );
              })}
            </ul>
          )}
        </Card>
      </div>

      <Modal
        open={all !== null}
        onClose={() => !all?.run && setAll(null)}
        closeLabel={t("common.close")}
        title={t("profiles.restore.undo_all.title")}
        footer={
          <>
            <Button variant="ghost" disabled={!!all?.run} onClick={() => setAll(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              disabled={!all?.plan || all.plan.steps.length === 0 || !!all.run}
              onClick={undoAll}
            >
              {t("profiles.restore.undo_all.go")}
            </Button>
          </>
        }
      >
        <div className="flex w-[min(480px,80vw)] flex-col gap-3">
          {shownAll?.run ? (
            <div className="flex flex-col gap-2" aria-live="polite">
              <p>
                {t("profiles.apply.running", {
                  done: shownAll.run.done,
                  total: shownAll.run.total,
                })}
              </p>
              <Meter
                value={shownAll.run.total ? shownAll.run.done / shownAll.run.total : 0}
                label={t("profiles.restore.undo_all")}
              />
            </div>
          ) : !shownAll?.plan ? (
            <Skeleton className="h-10 w-full" />
          ) : (
            <>
              <p>{t("profiles.restore.undo_all.body", { count: shownAll.plan.steps.length })}</p>
              <ul className="flex max-h-32 flex-wrap gap-1.5 overflow-auto">
                {shownAll.plan.steps.map((s) => (
                  <li key={s.id}>
                    <Badge tone="neutral" className={CHIP}>
                      {tweakLabel(t, s.id)}
                    </Badge>
                  </li>
                ))}
              </ul>
              {noBackupAll > 0 ? (
                <p className="text-xs text-warning">
                  {t("profiles.apply.nobackup", { count: noBackupAll })}
                </p>
              ) : null}
              {shownAll.plan.blocked.length > 0 ? (
                <p className="text-xs text-warning">
                  {t("profiles.restore.undo_all.blocked", { count: shownAll.plan.blocked.length })}
                </p>
              ) : null}
              <p className="text-xs text-muted-foreground">
                {t("profiles.restore.undo_all.others")}
              </p>
            </>
          )}
        </div>
      </Modal>
    </div>
  );
}
