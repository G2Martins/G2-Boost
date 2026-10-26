import {
  BotOff,
  BrainCircuit,
  ChevronDown,
  ListChecks,
  ListX,
  PackageOpen,
  RefreshCw,
  Trash,
  Undo2,
} from "lucide-react";
import { Fragment, useCallback, useEffect, useState } from "react";
import {
  applicable,
  RestartBadge,
  setTweak,
  TweakChanges,
  unknownNeedsAdmin,
  useTweakErrorText,
  useTweakStates,
} from "@/components/boost/tweak-kit";
import { ElevateButton } from "@/components/elevate-button";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
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
  AI_SET,
  DEBLOAT_LEVELS,
  type DebloatLevel,
  TWEAKS,
  type Tweak,
  tweakById,
  tweakDesc,
  tweakName,
  tweakWarn,
} from "@/data/catalogs";
import { backend, inTauri, type PreinstalledApp, type TweakState } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const DEBLOAT = TWEAKS.filter((tw) => tw.level);
const AI_ITEMS = AI_SET.map((id) => tweakById(id)).filter((tw): tw is Tweak => !!tw);
/** How deep a level goes: calm, the accent, caution. */
const LEVEL_DOT = {
  essential: "bg-success",
  recommended: "bg-primary",
  advanced: "bg-warning",
} as const;
/** Counted in "N de M": everything except what this PC does not have. */
const present = (s?: TweakState) => s?.status !== "unavailable" || unknownNeedsAdmin(s);

export function DebloatPage() {
  const { t } = useI18n();
  const errorText = useTweakErrorText();
  const { states, put } = useTweakStates();
  const [elevated, setElevated] = useState(false);
  const [restore, setRestore] = useState(true);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [open, setOpen] = useState<Set<string>>(new Set());
  const [ask, setAsk] = useState<string[] | null>(null);
  const [run, setRun] = useState<{ done: number; total: number } | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busyRow, setBusyRow] = useState<string | null>(null);

  useEffect(() => {
    backend.appInfo().then((i) => i && setElevated(i.elevated));
    backend.settingsGet().then((s) => s && setRestore(s.restore_before_preset));
  }, []);

  const can = (tw: Tweak) => applicable(tw, states?.[tw.id], elevated);
  const selected = DEBLOAT.filter((tw) => picked.has(tw.id) && can(tw)).map((tw) => tw.id);
  const total = DEBLOAT.filter((tw) => present(states?.[tw.id])).length;
  const on = DEBLOAT.filter((tw) => states?.[tw.id]?.status === "on").length;
  const aiTodo = AI_ITEMS.filter(can).map((tw) => tw.id);
  const aiOn = AI_ITEMS.filter((tw) => states?.[tw.id]?.status === "on").length;
  const aiTotal = AI_ITEMS.filter((tw) => present(states?.[tw.id])).length;

  const toggle = (set: Set<string>, id: string) => {
    const next = new Set(set);
    if (!next.delete(id)) next.add(id);
    return next;
  };

  const undoMany = async (ids: string[]) => {
    for (const id of [...ids].reverse()) {
      try {
        put(await setTweak(id, false));
      } catch (e) {
        setNote(errorText(e));
      }
    }
    toast(t("tweaks.toast.batch_undone", { count: ids.length }));
  };

  const applyIds = async (ids: string[], withRestore: boolean) => {
    setAsk(null);
    setNote(null);
    if (withRestore && elevated) {
      try {
        const out = await backend.restoreCreate(t("dash.restore.description"));
        setNote(out === "created" ? t("tweaks.restore.created") : t("tweaks.restore.skipped"));
      } catch (e) {
        setNote(errorText(e));
      }
    }
    const done: string[] = [];
    const errors: string[] = [];
    setRun({ done: 0, total: ids.length });
    for (const [i, id] of ids.entries()) {
      try {
        put(await setTweak(id, true));
        done.push(id);
      } catch (e) {
        errors.push(`${t(tweakName(id))}: ${errorText(e)}`);
      }
      setRun({ done: i + 1, total: ids.length });
    }
    setRun(null);
    setPicked(new Set());
    if (errors.length > 0) setNote(errors.join(" · "));
    if (done.length > 0) {
      toast(t("tweaks.toast.batch", { count: done.length }), {
        tone: "success",
        action: { label: t("tweaks.toast.undo"), run: () => void undoMany(done) },
      });
    }
  };

  const undo = async (tw: Tweak) => {
    setBusyRow(tw.id);
    try {
      put(await setTweak(tw.id, false));
      toast(t("tweaks.toast.undone", { name: t(tweakName(tw.id)) }));
    } catch (e) {
      setNote(errorText(e));
    }
    setBusyRow(null);
  };

  return (
    <>
      <PageHeader
        title={t("nav.debloat")}
        subtitle={t("debloat.subtitle")}
        actions={elevated ? null : <ElevateButton route="debloat" />}
      />
      <PageBody>
        <Card className="animate-rise flex flex-wrap items-center gap-x-6 gap-y-3 py-4">
          <div className="min-w-56 flex-1">
            <p className="font-display text-[18px] font-semibold tracking-wide tabular-nums">
              {t("debloat.applied_count", { on, total })}
            </p>
            <Meter
              value={on / Math.max(1, total)}
              className="mt-2 max-w-sm"
              label={t("debloat.applied_count", { on, total })}
            />
          </div>
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="sm"
              disabled={run !== null}
              onClick={() => setPicked(new Set(DEBLOAT.filter(can).map((tw) => tw.id)))}
            >
              <ListChecks className="size-4" />
              {t("debloat.select_all")}
            </Button>
            <Button
              variant="ghost"
              size="sm"
              disabled={run !== null || picked.size === 0}
              onClick={() => setPicked(new Set())}
            >
              <ListX className="size-4" />
              {t("debloat.select_none")}
            </Button>
          </div>
          <Button
            variant="primary"
            size="lg"
            tactile
            disabled={selected.length === 0 || run !== null}
            onClick={() => setAsk(selected)}
          >
            {t("debloat.apply", { count: selected.length })}
          </Button>
          {run ? (
            <p className="w-full text-xs text-muted-foreground">{t("debloat.applying", run)}</p>
          ) : null}
          {note ? <p className="w-full text-xs text-foreground/90">{note}</p> : null}
          {elevated ? null : (
            <p className="w-full text-xs text-warning">{t("debloat.elevate.hint")}</p>
          )}
        </Card>

        <Card className="halo frame-ticks animate-rise" style={rise(1)}>
          <div className="flex flex-wrap items-start gap-4">
            <span className="flex size-11 shrink-0 items-center justify-center rounded-lg bg-primary/12 text-primary ring-1 ring-primary/25 ring-inset">
              <BrainCircuit className="size-5" strokeWidth={1.8} />
            </span>
            <div className="min-w-0 flex-1">
              <h2 className="font-display text-[16px] font-semibold tracking-wide">
                {t("debloat.ai.title")}
              </h2>
              <p className="mt-1 max-w-2xl text-xs leading-relaxed text-muted-foreground">
                {t("debloat.ai.body")}
              </p>
            </div>
            <Button
              variant="outline"
              disabled={aiTodo.length === 0 || run !== null}
              onClick={() => setAsk(aiTodo)}
            >
              <BotOff className="size-4" />
              {t("debloat.ai.apply")}
            </Button>
          </div>
          <ul className="mt-4 flex flex-wrap gap-2">
            {AI_ITEMS.map((tw) => {
              const s = states?.[tw.id];
              const Icon = tw.icon;
              return (
                <li
                  key={tw.id}
                  className="flex items-center gap-2 rounded-md border border-border bg-black/20 px-2.5 py-1.5 text-xs"
                >
                  <span
                    aria-hidden
                    className={cn(
                      "size-1.5 rounded-full transition-colors duration-200 ease-out-expo",
                      s?.status === "on"
                        ? "bg-success shadow-[0_0_6px_hsl(var(--success))]"
                        : s?.status === "partial"
                          ? "bg-warning"
                          : unknownNeedsAdmin(s)
                            ? "ring-1 ring-muted-foreground"
                            : "bg-muted-foreground/40",
                    )}
                  />
                  <Icon className="size-3.5 text-muted-foreground" />
                  {t(tweakName(tw.id))}
                  <span className="sr-only">
                    <StateText s={s} />
                  </span>
                </li>
              );
            })}
          </ul>
          <p className="mt-3 flex flex-wrap gap-x-2 text-[11px] text-muted-foreground">
            <span className="tabular-nums">
              {t("debloat.ai.count", { on: aiOn, total: aiTotal })}
            </span>
            <span aria-hidden>·</span>
            <button
              type="button"
              className="underline decoration-dotted underline-offset-2 hover:text-foreground"
              onClick={() => document.getElementById("debloat-apps")?.scrollIntoView()}
            >
              {t("debloat.ai.copilot_app")}
            </button>
          </p>
        </Card>

        <Card className="frame-ticks animate-rise overflow-x-auto p-0" style={rise(2)}>
          <table className="w-full border-separate border-spacing-0 text-sm">
            <thead className="sr-only">
              <tr>
                <th scope="col">{t("debloat.select_all")}</th>
                <th scope="col">{t("debloat.col.item")}</th>
                <th scope="col">{t("debloat.col.state")}</th>
                <th scope="col">{t("tweaks.changes")}</th>
              </tr>
            </thead>
            {DEBLOAT_LEVELS.map((level) => (
              <LevelGroup
                key={level}
                level={level}
                items={DEBLOAT.filter((tw) => tw.level === level)}
                states={states}
                elevated={elevated}
                picked={picked}
                open={open}
                busy={run !== null}
                busyRow={busyRow}
                onPick={(id) => setPicked((p) => toggle(p, id))}
                onOpen={(id) => setOpen((o) => toggle(o, id))}
                onUndo={undo}
              />
            ))}
          </table>
        </Card>

        <PreinstalledApps index={3} />
      </PageBody>

      <Modal
        open={ask !== null}
        onClose={() => setAsk(null)}
        title={t("debloat.confirm.title", { count: ask?.length ?? 0 })}
        closeLabel={t("common.cancel")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setAsk(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" onClick={() => ask && void applyIds(ask, restore)}>
              {t("debloat.confirm.go")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("debloat.confirm.body")}</p>
        <ul className="mt-3 max-h-60 space-y-1.5 overflow-y-auto pr-1">
          {(ask ?? []).map((id) => {
            const tw = tweakById(id);
            const Icon = tw?.icon;
            return (
              <li key={id} className="flex items-center gap-2 text-[13px]">
                {Icon ? <Icon className="size-4 shrink-0 text-muted-foreground" /> : null}
                <span className="min-w-0 flex-1 truncate">{t(tweakName(id))}</span>
                <RestartBadge restart={tw?.restart} />
              </li>
            );
          })}
        </ul>
        {(ask ?? []).some((id) => tweakById(id)?.restart) ? (
          <p className="mt-3 text-xs text-warning">{t("debloat.confirm.restart")}</p>
        ) : null}
        {elevated ? (
          <label className="mt-3 flex items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={restore}
              onChange={(e) => setRestore(e.target.checked)}
              className="size-4 accent-primary"
            />
            {t("debloat.confirm.restore")}
          </label>
        ) : null}
      </Modal>
    </>
  );
}

function StateText({ s }: { s?: TweakState }) {
  const { t } = useI18n();
  if (!s) return <>{inTauri ? "" : "—"}</>;
  if (unknownNeedsAdmin(s)) return <>{t("boost.state.unknown_admin")}</>;
  if (s.status === "on") return <>{t("debloat.state.on")}</>;
  if (s.status === "off") return <>{t("debloat.state.off")}</>;
  return <>{t(`boost.state.${s.status}` as const)}</>;
}

function StateChip({ s, loading }: { s?: TweakState; loading: boolean }) {
  if (loading) return <Skeleton className="h-4 w-20" />;
  const tone = s?.status === "on" ? "success" : s?.status === "partial" ? "warning" : "neutral";
  return (
    <Badge tone={s ? tone : undefined} className={s ? "" : "text-muted-foreground"}>
      <StateText s={s} />
    </Badge>
  );
}

function LevelGroup({
  level,
  items,
  states,
  elevated,
  picked,
  open,
  busy,
  busyRow,
  onPick,
  onOpen,
  onUndo,
}: {
  level: DebloatLevel;
  items: Tweak[];
  states: Record<string, TweakState> | null;
  elevated: boolean;
  picked: Set<string>;
  open: Set<string>;
  busy: boolean;
  busyRow: string | null;
  onPick: (id: string) => void;
  onOpen: (id: string) => void;
  onUndo: (tw: Tweak) => void;
}) {
  const { t } = useI18n();
  const on = items.filter((tw) => states?.[tw.id]?.status === "on").length;
  const total = items.filter((tw) => present(states?.[tw.id])).length;
  return (
    <tbody>
      <tr>
        <th
          colSpan={4}
          scope="colgroup"
          className="border-b border-border bg-black/25 px-5 pt-4 pb-3 text-left font-normal"
        >
          <span className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
              <span aria-hidden className={cn("size-2 rounded-full", LEVEL_DOT[level])} />
              {t(`debloat.level.${level}` as const)}
            </span>
            <Badge tone="neutral">{t("debloat.applied_count", { on, total })}</Badge>
            <span className="text-xs text-muted-foreground">
              {t(`debloat.level.${level}.hint` as I18nKey)}
            </span>
          </span>
        </th>
      </tr>
      {items.map((tw) => {
        const s = states?.[tw.id];
        const can = applicable(tw, s, elevated);
        const expanded = open.has(tw.id);
        const Icon = tw.icon;
        const undoable = (s?.status === "on" || s?.status === "partial") && (!tw.admin || elevated);
        return (
          <Fragment key={tw.id}>
            <tr
              className={cn(
                "align-top transition-colors duration-200 ease-out-expo hover:bg-foreground/2.5",
                s?.status === "unavailable" && !unknownNeedsAdmin(s) && "opacity-55",
              )}
            >
              <td className="w-10 border-b border-border/70 py-3 pl-5">
                <input
                  type="checkbox"
                  checked={can && picked.has(tw.id)}
                  disabled={!can || busy}
                  onChange={() => onPick(tw.id)}
                  aria-label={t("debloat.select", { name: t(tweakName(tw.id)) })}
                  className="mt-0.5 size-4 accent-primary"
                />
              </td>
              <td className="border-b border-border/70 py-3 pr-3 pl-1">
                <div className="flex gap-3">
                  <Icon className="mt-0.5 size-4.5 shrink-0 text-foreground/80" strokeWidth={1.8} />
                  <div className="min-w-0">
                    <p className="flex flex-wrap items-center gap-1.5">
                      <span className="text-[13px] font-semibold">{t(tweakName(tw.id))}</span>
                      {tw.admin && !elevated ? (
                        <Badge tone="neutral">{t("common.admin_required")}</Badge>
                      ) : null}
                      <RestartBadge restart={tw.restart} />
                    </p>
                    <p className="mt-0.5 max-w-3xl text-xs leading-relaxed text-muted-foreground">
                      {t(tweakDesc(tw.id))}
                    </p>
                    {s?.detail === "reboot_pending" ? (
                      <p className="mt-1 text-[11px] text-warning">
                        {t("boost.state.reboot_pending")}
                      </p>
                    ) : null}
                  </div>
                </div>
              </td>
              <td className="w-44 border-b border-border/70 py-3 pr-3">
                <StateChip s={s} loading={inTauri && states === null} />
              </td>
              <td className="w-px border-b border-border/70 py-2 pr-4 text-right whitespace-nowrap">
                {undoable ? (
                  <Button
                    variant="ghost"
                    size="sm"
                    disabled={busy || busyRow !== null}
                    onClick={() => onUndo(tw)}
                  >
                    <Undo2 className="size-3.5" />
                    {t("debloat.undo")}
                  </Button>
                ) : null}
                <Button
                  variant="ghost"
                  size="sm"
                  aria-expanded={expanded}
                  aria-controls={`changes-${tw.id}`}
                  onClick={() => onOpen(tw.id)}
                >
                  <ChevronDown
                    className={cn(
                      "size-3.5 motion-safe:transition-transform motion-safe:duration-200",
                      expanded && "rotate-180",
                    )}
                  />
                  {expanded ? t("tweaks.changes.hide") : t("tweaks.changes")}
                </Button>
              </td>
            </tr>
            {expanded ? (
              <tr id={`changes-${tw.id}`}>
                <td />
                <td colSpan={3} className="border-b border-border/70 pr-4 pb-3 pl-1">
                  <TweakChanges id={tw.id} />
                  <p className="mt-2 text-[11px] text-muted-foreground">
                    <span className="font-semibold text-foreground/80">
                      {t("common.not_recommended")}
                    </span>{" "}
                    {t(tweakWarn(tw.id))}
                  </p>
                </td>
              </tr>
            ) : null}
          </Fragment>
        );
      })}
    </tbody>
  );
}

/** Store apps from the core's allowlist: preview, confirmation token, removal for this user. */
function PreinstalledApps({ index }: { index: number }) {
  const { t } = useI18n();
  const errorText = useTweakErrorText();
  const [apps, setApps] = useState<PreinstalledApp[] | null>(null);
  const [loading, setLoading] = useState(inTauri);
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [ask, setAsk] = useState(false);
  const [working, setWorking] = useState(false);
  const [failures, setFailures] = useState<string[]>([]);

  const load = useCallback(async () => {
    setLoading(true);
    const list = await backend.debloatApps();
    if (list) setApps(list);
    setLoading(false);
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  const chosen = (apps ?? []).filter((a) => picked.has(a.id));
  const remove = async () => {
    setAsk(false);
    setWorking(true);
    setFailures([]);
    try {
      const token = await backend.debloatAppsConfirm(chosen.map((a) => a.id));
      const out = await backend.debloatAppsRemove(token.token, token.ids);
      const ok = out.filter((r) => r.ok).length;
      setFailures(
        out
          .filter((r) => !r.ok)
          .map((r) => t("debloat.apps.failed", { name: r.name || r.id, error: r.error ?? "" })),
      );
      if (ok > 0) toast(t("debloat.apps.removed", { count: ok }), { tone: "success" });
    } catch (e) {
      setFailures([errorText(e)]);
    }
    setPicked(new Set());
    setWorking(false);
    await load();
  };

  return (
    <Card id="debloat-apps" className="animate-rise scroll-mt-4" style={rise(index)}>
      <CardTitle
        help={t("debloat.apps.help")}
        right={
          <Button
            variant="outline"
            size="sm"
            disabled={loading || working || !inTauri}
            onClick={() => void load()}
            aria-label={t("debloat.apps.refresh")}
          >
            <RefreshCw className={cn("size-4", loading && "motion-safe:animate-spin")} />
          </Button>
        }
      >
        <PackageOpen className="size-4" />
        {t("debloat.apps.title")}
      </CardTitle>
      <p className="-mt-2 mb-3 text-xs text-muted-foreground">{t("debloat.apps.note")}</p>
      {loading && apps === null ? (
        <div className="space-y-2">
          {[0, 1, 2].map((k) => (
            <Skeleton key={k} className="h-10 w-full" />
          ))}
        </div>
      ) : (apps ?? []).length === 0 ? (
        <EmptyState icon={<PackageOpen className="size-5" />} title={t("debloat.apps.empty")} />
      ) : (
        <ul className="divide-y divide-border/70">
          {(apps ?? []).map((a) => (
            <li key={a.id} className="flex items-center gap-3 py-2.5">
              <input
                type="checkbox"
                checked={picked.has(a.id)}
                disabled={working}
                onChange={() =>
                  setPicked((p) => {
                    const next = new Set(p);
                    if (!next.delete(a.id)) next.add(a.id);
                    return next;
                  })
                }
                aria-label={t("debloat.select", { name: a.name })}
                className="size-4 accent-primary"
              />
              <div className="min-w-0 flex-1">
                <p className="flex flex-wrap items-center gap-1.5">
                  <span className="truncate text-sm font-semibold">{a.name}</span>
                  <Badge tone="neutral">{t(`debloat.app.kind.${a.kind}` as I18nKey)}</Badge>
                </p>
                <p className="mt-0.5 truncate font-mono text-[11px] text-muted-foreground">
                  {a.family}
                </p>
              </div>
            </li>
          ))}
        </ul>
      )}
      {failures.length > 0 ? (
        <ul className="mt-3 space-y-1 text-xs text-destructive">
          {failures.map((f) => (
            <li key={f}>{f}</li>
          ))}
        </ul>
      ) : null}
      {(apps ?? []).length > 0 ? (
        <div className="mt-4 flex justify-end">
          <Button
            variant="danger"
            disabled={chosen.length === 0 || working}
            onClick={() => setAsk(true)}
          >
            <Trash className="size-4" />
            {t("debloat.apps.remove", { count: chosen.length })}
          </Button>
        </div>
      ) : null}
      <Modal
        open={ask}
        onClose={() => setAsk(false)}
        title={t("debloat.apps.confirm.title", { count: chosen.length })}
        closeLabel={t("common.cancel")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setAsk(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="danger" onClick={() => void remove()}>
              {t("debloat.apps.confirm.go")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("debloat.apps.confirm.body")}</p>
        <ul className="mt-3 max-h-60 space-y-1 overflow-y-auto">
          {chosen.map((a) => (
            <li key={a.id} className="truncate text-[13px]">
              {a.name}
            </li>
          ))}
        </ul>
      </Modal>
    </Card>
  );
}
