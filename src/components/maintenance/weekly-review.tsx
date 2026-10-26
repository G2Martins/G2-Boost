import {
  ArchiveRestore,
  CalendarClock,
  Clock,
  FileCheck2,
  HardDrive,
  type LucideIcon,
  ScanSearch,
  ShieldCheck,
  Square,
  TriangleAlert,
  Wrench,
} from "lucide-react";
import { useEffect, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, HelpHint, Modal } from "@/components/ui/primitives";
import {
  backend,
  inTauri,
  type MaintPlan,
  type MaintPlanOutcome,
  type MaintPlanRunning,
  type MaintStep,
  type MaintStepOutcome,
  type MaintStepState,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Same order as `repair::plan_steps` (the core decides; this only lays out the stations). */
const PLAN_STEPS: Record<MaintPlan, MaintStep[]> = {
  weekly: ["restore_point", "drive_scan", "sfc", "dism_scan"],
  full_repair: ["dism_restore", "sfc"],
};

const STEP_ICON: Record<MaintStep, LucideIcon> = {
  restore_point: ArchiveRestore,
  drive_scan: HardDrive,
  sfc: FileCheck2,
  dism_scan: ScanSearch,
  dism_restore: Wrench,
};

const STEP_TIME: Record<MaintStep, I18nKey> = {
  restore_point: "maint3.time.restore_point",
  drive_scan: "maint2.time.scan",
  sfc: "maintenance.sfc.time",
  dism_scan: "maintenance.dism_scan.time",
  dism_restore: "maintenance.dism_restore.time",
};

const STATE_TONE: Record<MaintStepState, "success" | "warning" | "danger" | "neutral"> = {
  ok: "success",
  attention: "warning",
  failed: "danger",
  skipped: "neutral",
  cancelled: "neutral",
};

const LAMP: Record<MaintStepState, string> = {
  ok: "bg-success",
  attention: "bg-warning",
  failed: "bg-destructive",
  skipped: "bg-foreground/30",
  cancelled: "bg-foreground/30",
};

/** Codes worth a sentence of their own; anything else shows the raw code. */
export function codeKey(code: number): I18nKey | null {
  if (code === 1460) return "maintenance.code.timeout";
  if ([0x800f081f, 0x800f0906, 0x800f0907, 0x800f0950].includes(code))
    return "maintenance.code.source";
  if (code === 0x800f082f) return "maintenance.code.pending";
  return null;
}

export const hex = (code: number) => `0x${code.toString(16).toUpperCase().padStart(8, "0")}`;

export function elapsed(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

type Running = MaintPlanRunning & { since: number; adopted?: boolean };

/**
 * "Revisão semanal": one click chains restore point → scan of C: → SFC → DISM scan inside the
 * app's single servicing slot. A repairable image is offered as the full repair (DISM
 * RestoreHealth → SFC), never started on its own.
 */
export function WeeklyReview({
  elevated,
  blocked,
  index,
  onRunning,
  onFinished,
}: {
  elevated: boolean;
  /** Another tool owns the servicing slot (or the app is not elevated / in the browser). */
  blocked: boolean;
  index: number;
  onRunning: (on: boolean) => void;
  onFinished: () => void;
}) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [history, setHistory] = useState<MaintPlanOutcome[] | null>(null);
  const [running, setRunning] = useState<Running | null>(null);
  const [confirm, setConfirm] = useState<MaintPlan | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [, tick] = useState(0);

  useEffect(() => {
    backend.maintPlanHistory().then((h) => setHistory(h ?? []));
    backend.maintPlanStatus().then((p) => {
      if (p) setRunning({ ...p, since: Date.now(), adopted: true });
    });
    let off: (() => void) | null = null;
    backend
      .on<MaintPlanRunning>("maintenance:plan-progress", (p) =>
        setRunning((r) => (r && r.plan === p.plan ? { ...r, ...p } : r)),
      )
      .then((u) => {
        off = u;
      });
    return () => off?.();
  }, []);

  useEffect(() => onRunning(running !== null), [running, onRunning]);

  useEffect(() => {
    if (!running) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [running]);

  // Started before this page mounted: nobody awaits it here, so poll for the end.
  const adopted = running?.adopted === true;
  // biome-ignore lint/correctness/useExhaustiveDependencies: onFinished only refreshes the page's history
  useEffect(() => {
    if (!adopted) return;
    const id = window.setInterval(async () => {
      if (!(await backend.maintPlanStatus())) {
        setRunning(null);
        setHistory((await backend.maintPlanHistory()) ?? []);
        onFinished();
      }
    }, 3000);
    return () => window.clearInterval(id);
  }, [adopted]);

  const start = async (plan: MaintPlan) => {
    setConfirm(null);
    setError(null);
    const steps = PLAN_STEPS[plan];
    setRunning({
      plan,
      step: steps[0] as MaintStep,
      index: 0,
      total: steps.length,
      percent: null,
      done: [],
      since: Date.now(),
    });
    try {
      const out = await backend.maintPlanRun(plan, t("maint3.review.restore_label"));
      setHistory((h) => [...(h ?? []), out]);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg === "busy" ? t("maintenance.busy") : errorText(e));
    } finally {
      setRunning(null);
      onFinished();
    }
  };

  const last = history?.at(-1) ?? null;
  const plan: MaintPlan = running?.plan ?? last?.plan ?? "weekly";
  const shown = running ? running.done : last?.plan === plan ? last.steps : [];
  const lastWeekly = history ? [...history].reverse().find((o) => o.plan === "weekly") : undefined;
  const days = lastWeekly ? Math.floor((Date.now() / 1000 - lastWeekly.at) / 86400) : null;
  const flagged = last
    ? last.steps.filter((s) => s.state === "attention" || s.state === "failed")
    : [];
  const summary = !last
    ? null
    : last.steps.some((s) => s.state === "cancelled")
      ? { tone: "neutral" as const, text: t("maint3.review.done.cancelled") }
      : flagged.length === 0
        ? { tone: "success" as const, text: t("maint3.review.done.ok") }
        : {
            tone: "warning" as const,
            text:
              flagged.length === 1
                ? t("maint3.review.done.one")
                : t("maint3.review.done.many", { n: flagged.length }),
          };
  const idle = !running;
  const offer = idle && last?.plan === "weekly" && last.offer_repair;

  return (
    <Card className="frame-ticks animate-rise relative flex flex-col gap-5" style={rise(index)}>
      <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_auto] lg:items-end">
        <div className="min-w-0">
          <p className="text-[10px] font-semibold uppercase tracking-[0.14em] text-primary">
            {t("maint3.review.eyebrow")}
          </p>
          <h2 className="mt-1 font-display text-[20px] font-semibold tracking-wide">
            {t("maint3.review.title")}
          </h2>
          <p className="mt-1.5 max-w-2xl text-xs leading-relaxed text-muted-foreground">
            {t("maint3.review.body")}
          </p>
          <p className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
            <span className="inline-flex items-center gap-1.5">
              <Clock className="size-3" />
              {t("maint3.review.time")}
            </span>
            <span className="inline-flex items-center gap-1.5">
              <CalendarClock className="size-3" />
              {days === null
                ? t("maint3.review.never")
                : t("maint3.review.last", {
                    when:
                      days <= 0
                        ? t("maint3.review.today")
                        : days === 1
                          ? t("maint3.review.yesterday")
                          : t("maint3.review.days", { n: days }),
                  })}
            </span>
            {days !== null && days >= 7 && idle ? (
              <span className="text-warning">{t("maint3.review.due")}</span>
            ) : null}
          </p>
        </div>

        <div className="flex flex-wrap items-center gap-2 lg:flex-col lg:items-end">
          {running ? (
            <div className="flex items-center gap-3">
              <span className="num text-xs text-muted-foreground">
                {t("maint3.review.running", { n: running.index + 1, total: running.total })}
                {" · "}
                {elapsed(Date.now() - running.since)}
              </span>
              <Button variant="ghost" size="sm" onClick={() => void backend.repairCancel()}>
                <Square className="size-3.5" />
                {t("common.stop")}
              </Button>
            </div>
          ) : (
            <Button
              variant="primary"
              size="lg"
              tactile
              disabled={blocked}
              onClick={() => setConfirm("weekly")}
            >
              <ShieldCheck className="size-4" />
              {t("maint3.review.start")}
            </Button>
          )}
          <span className="inline-flex items-center gap-1.5">
            <Button
              variant="outline"
              size="sm"
              disabled={blocked || running !== null}
              onClick={() => setConfirm("full_repair")}
            >
              <Wrench className="size-3.5" />
              {t("maint3.repair.button")}
            </Button>
            <HelpHint text={t("maintenance.order.hint")} />
          </span>
        </div>
      </div>

      {plan === "full_repair" ? (
        <p className="-mb-3 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
          {t("maint3.repair.button")}
        </p>
      ) : null}
      <ol
        className={cn(
          "grid grid-cols-1 gap-2 sm:grid-cols-2",
          PLAN_STEPS[plan].length > 2 && "xl:grid-cols-4",
        )}
      >
        {PLAN_STEPS[plan].map((step, i) => (
          <Station
            key={step}
            step={step}
            n={i + 1}
            outcome={shown[i] ?? null}
            live={running && running.index === i ? running.percent : undefined}
            queued={running !== null && i > running.index}
          />
        ))}
      </ol>

      {idle && summary ? (
        <p className="-mt-1 flex flex-wrap items-center gap-2 text-xs">
          <Badge tone={summary.tone}>
            {t(last?.plan === "full_repair" ? "maint3.repair.button" : "maint3.review.title")}
          </Badge>
          <span className="text-foreground/85">{summary.text}</span>
        </p>
      ) : null}

      {offer ? (
        <div className="flex flex-wrap items-center gap-3 rounded-md bg-warning/8 p-3 ring-1 ring-warning/25 ring-inset">
          <TriangleAlert className="size-4 shrink-0 text-warning" />
          <p className="min-w-0 flex-1 text-xs leading-relaxed text-foreground/90">
            {t("maint3.review.offer")}
          </p>
          <Button
            variant="primary"
            size="sm"
            disabled={blocked}
            onClick={() => setConfirm("full_repair")}
          >
            <Wrench className="size-3.5" />
            {t("maint3.repair.button")}
          </Button>
        </div>
      ) : null}
      {error ? <p className="text-xs text-destructive">{error}</p> : null}

      <Modal
        open={confirm !== null}
        onClose={() => setConfirm(null)}
        title={t(
          confirm === "full_repair" ? "maint3.repair.confirm.title" : "maint3.review.confirm.title",
        )}
        closeLabel={t("common.cancel")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setConfirm(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              disabled={!inTauri || !elevated}
              onClick={() => confirm && void start(confirm)}
            >
              {t(
                confirm === "full_repair" ? "maint3.repair.confirm.go" : "maint3.review.confirm.go",
              )}
            </Button>
          </>
        }
      >
        {confirm === "full_repair" ? (
          <p>{t("maint3.repair.confirm.body")}</p>
        ) : (
          <div className="space-y-2">
            <p>{t("maint3.review.confirm.body")}</p>
            <p className="text-xs text-muted-foreground">{t("maint3.review.confirm.restore")}</p>
          </div>
        )}
      </Modal>
    </Card>
  );
}

/** What a finished step says, from its own verdict family. */
function verdictText(o: MaintStepOutcome, t: ReturnType<typeof useI18n>["t"]): string {
  const family = o.step === "drive_scan" ? "maint2.verdict" : "maintenance.verdict";
  for (const k of [`maint3.verdict.${o.verdict}`, `${family}.${o.verdict}`]) {
    if (t(k as I18nKey) !== k) return t(k as I18nKey);
  }
  return o.verdict;
}

/** One line under a step that needs a look (the full-repair offer covers the image). */
function stepDetail(o: MaintStepOutcome, t: ReturnType<typeof useI18n>["t"]): string | null {
  if (o.state === "failed" && o.code) {
    const k = codeKey(o.code);
    return k ? t(k) : t("maintenance.code.other", { code: hex(o.code) });
  }
  if (o.verdict === "protection_off") return t("maint3.detail.protection_off");
  if (o.step === "drive_scan" && ["needs_spot_fix", "needs_offline", "retry"].includes(o.verdict))
    return t(`maint2.detail.${o.verdict}` as I18nKey);
  if (o.step === "sfc" && (o.repaired > 0 || o.unrepaired > 0))
    return t("maintenance.detail.sfc", { repaired: o.repaired, unrepaired: o.unrepaired });
  if (o.step.startsWith("dism") && o.verdict === "non_repairable")
    return t("maintenance.detail.dism_bad");
  return null;
}

function Station({
  step,
  n,
  outcome,
  live,
  queued,
}: {
  step: MaintStep;
  n: number;
  outcome: MaintStepOutcome | null;
  /** Running now: its percent (`null` = no percentage from Windows). */
  live: number | null | undefined;
  queued: boolean;
}) {
  const { t } = useI18n();
  const Icon = STEP_ICON[step];
  const now = live !== undefined;
  const detail = outcome ? stepDetail(outcome, t) : null;
  return (
    <li
      className={cn(
        "relative flex flex-col gap-2 rounded-lg border border-border bg-background/40 p-3 transition-colors duration-300",
        now && "border-primary/45 shadow-[inset_0_1px_0_hsl(var(--primary)/0.55)]",
        queued && "opacity-60",
      )}
    >
      <div className="flex items-center gap-2">
        <span className="num text-[11px] text-muted-foreground">{String(n).padStart(2, "0")}</span>
        <span
          aria-hidden
          className={cn(
            "ml-auto size-2 rounded-full",
            now
              ? "bg-primary shadow-[0_0_8px_hsl(var(--primary)/0.8)]"
              : outcome
                ? LAMP[outcome.state]
                : "bg-foreground/15",
          )}
        />
      </div>
      <p className="flex items-center gap-2 text-[13px] font-semibold">
        <Icon className="size-4 shrink-0 text-muted-foreground" />
        {t(`maint3.step.${step}` as I18nKey)}
      </p>
      <p className="font-mono text-[10.5px] leading-snug text-muted-foreground">
        {t(`maint3.runs.${step}` as I18nKey)}
      </p>
      <p className="inline-flex items-center gap-1.5 text-[11px] text-muted-foreground">
        <Clock className="size-3" />
        {t(STEP_TIME[step])}
      </p>
      <div className="mt-auto border-t border-border pt-2 text-xs">
        {now ? (
          <div className="flex items-center gap-2">
            <span
              className="relative block h-1.5 flex-1 overflow-hidden rounded-full bg-border"
              role="progressbar"
              aria-label={t(`maint3.step.${step}` as I18nKey)}
              aria-valuemin={0}
              aria-valuemax={100}
              aria-valuenow={live ?? undefined}
            >
              {live !== null ? (
                <span
                  className="block h-full rounded-full bg-primary transition-[width] duration-300"
                  style={{ width: `${live}%` }}
                />
              ) : (
                <span className="absolute inset-y-0 w-1/3 rounded-full bg-primary/70 motion-safe:animate-sweep" />
              )}
            </span>
            <span className="num text-[11px] text-muted-foreground">
              {live !== null ? `${live}%` : t("maint3.state.running")}
            </span>
          </div>
        ) : outcome ? (
          <Badge tone={STATE_TONE[outcome.state]}>{verdictText(outcome, t)}</Badge>
        ) : (
          <span className="text-muted-foreground/70">
            {queued ? t("maint3.state.pending") : t("maintenance.never")}
          </span>
        )}
      </div>
      {detail ? <p className="text-[11px] leading-relaxed text-foreground/85">{detail}</p> : null}
    </li>
  );
}
