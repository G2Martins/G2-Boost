import { useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { Badge, Button, Meter, Modal, Skeleton } from "@/components/ui/primitives";
import { backend, type ProfilePlan, type ProfileStep, type ProfileStepResult } from "@/lib/backend";
import type { StateMap } from "@/lib/boost-hooks";
import { useI18n } from "@/lib/i18n";
import { type Entry, tweakLabel } from "./shared";

export type Applied = { entry: Entry; steps: ProfileStep[]; results: ProfileStepResult[] };

type Outcome = {
  ok: number;
  total: number;
  failed: { id: string; text: string }[];
  restore: string | null;
  /** Steps that needed administrator and did not run. */
  blocked: number;
};

const CHIP = "normal-case tracking-normal text-[11px]";

/**
 * Preview, then run. The plan comes from the core (what is on, what is off, what needs admin);
 * the steps run one by one through the normal tweak flow, so each one backs up first and the
 * progress is real. The caller keys this by profile, so every opening starts fresh.
 */
export function ApplyModal({
  entry,
  open,
  states,
  elevated,
  restoreBefore,
  onClose,
  onDone,
}: {
  entry: Entry;
  open: boolean;
  states: StateMap | null;
  elevated: boolean;
  restoreBefore: boolean;
  onClose: () => void;
  onDone: (a: Applied) => void;
}) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [offOthers, setOffOthers] = useState(
    entry.off === "always" || (entry.off === "ask" && entry.offDefault),
  );
  const [plan, setPlan] = useState<ProfilePlan | null>(null);
  const [planErr, setPlanErr] = useState<string | null>(null);
  const [run, setRun] = useState<{ done: number; total: number } | null>(null);
  const [outcome, setOutcome] = useState<Outcome | null>(null);

  useEffect(() => {
    let alive = true;
    setPlan(null);
    setPlanErr(null);
    backend
      .profilesPlan(entry.ids, entry.fx, offOthers)
      .then((p) => alive && setPlan(p))
      .catch((e) => alive && setPlanErr(errorText(e)));
    return () => {
      alive = false;
    };
  }, [entry, offOthers, errorText]);

  const apply = async () => {
    if (!plan || plan.steps.length === 0) return;
    const steps = plan.steps;
    setRun({ done: 0, total: steps.length });
    let restore: string | null = null;
    if (restoreBefore && elevated) {
      try {
        const made = await backend.restoreCreate(t("dash.restore.description"));
        restore =
          made === "created"
            ? t("profiles.apply.restore_created")
            : t("profiles.apply.restore_skipped");
      } catch (e) {
        restore = errorText(e);
      }
    }
    const results: ProfileStepResult[] = [];
    for (const [i, step] of steps.entries()) {
      try {
        results.push(await backend.profilesStep(step, entry.name));
      } catch (e) {
        const error = e instanceof Error ? e.message : String(e);
        results.push({ id: step.id, op: step.op, ok: false, error, state: null });
      }
      setRun({ done: i + 1, total: steps.length });
    }
    setRun(null);
    setOutcome({
      ok: results.filter((r) => r.ok).length,
      total: results.length,
      failed: results
        .filter((r) => !r.ok)
        .map((r) => ({ id: r.id, text: errorText(new Error(r.error ?? "")) })),
      restore,
      blocked: plan.blocked.length,
    });
    onDone({ entry, steps, results });
  };

  const turnOn = plan?.steps.filter((s) => s.op === "on") ?? [];
  const turnOff = plan?.steps.filter((s) => s.op === "off") ?? [];
  // Turning one off without a saved original writes the Windows default: say so before it happens.
  const noBackup = turnOff.filter((s) => states && !states[s.id]?.has_backup).length;
  const busy = run !== null;
  const close = () => {
    if (!busy) onClose();
  };

  return (
    <Modal
      open={open}
      onClose={close}
      closeLabel={t("common.close")}
      title={t("profiles.apply.title", { name: entry.name })}
      footer={
        outcome ? (
          <Button variant="secondary" onClick={onClose}>
            {t("common.close")}
          </Button>
        ) : (
          <>
            <Button variant="ghost" disabled={busy} onClick={close}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              disabled={busy || !plan || plan.steps.length === 0}
              onClick={apply}
            >
              {t("profiles.apply.go")}
            </Button>
          </>
        )
      }
    >
      <div className="flex w-[min(520px,80vw)] flex-col gap-3">
        {outcome ? (
          <div className="flex flex-col gap-2" aria-live="polite">
            <p className="font-semibold">
              {t("profiles.apply.done", { ok: outcome.ok, total: outcome.total })}
            </p>
            {outcome.restore ? (
              <p className="text-xs text-muted-foreground">{outcome.restore}</p>
            ) : null}
            {outcome.failed.length > 0 ? (
              <div className="flex flex-col gap-1">
                <p className="text-xs text-warning">
                  {t("profiles.apply.failed", { count: outcome.failed.length })}
                </p>
                <ul className="flex flex-col gap-0.5 text-xs">
                  {outcome.failed.map((f) => (
                    <li key={f.id}>
                      <span className="font-semibold">{tweakLabel(t, f.id)}</span>
                      <span className="text-muted-foreground">: {f.text}</span>
                    </li>
                  ))}
                </ul>
              </div>
            ) : null}
            {outcome.blocked > 0 ? (
              <div className="flex flex-col items-start gap-2">
                <p className="text-xs text-warning">
                  {t("profiles.apply.blocked", { count: outcome.blocked })}
                </p>
                {!elevated ? <ElevateButton route="profiles" /> : null}
              </div>
            ) : null}
            <p className="text-xs text-muted-foreground">{t("profiles.apply.safe")}</p>
          </div>
        ) : run ? (
          <div className="flex flex-col gap-2" aria-live="polite">
            <p>{t("profiles.apply.running", { done: run.done, total: run.total })}</p>
            <Meter value={run.total ? run.done / run.total : 0} label={t("profiles.apply.go")} />
          </div>
        ) : planErr ? (
          <p className="text-destructive">{planErr}</p>
        ) : !plan ? (
          <div className="flex flex-col gap-2" role="status">
            <span className="sr-only">{t("profiles.apply.loading")}</span>
            <Skeleton className="h-4 w-2/3" />
            <Skeleton className="h-4 w-1/2" />
          </div>
        ) : (
          <>
            {entry.off === "ask" ? (
              <label className="flex cursor-pointer items-start gap-2.5 rounded-md border border-border p-3">
                <input
                  type="checkbox"
                  className="mt-0.5 size-4 accent-primary"
                  checked={offOthers}
                  onChange={(e) => setOffOthers(e.target.checked)}
                />
                <span>
                  <span className="block text-[13px] font-semibold">
                    {t("profiles.apply.off_others")}
                  </span>
                  <span className="block text-xs text-muted-foreground">
                    {t("profiles.apply.off_others.hint")}
                  </span>
                </span>
              </label>
            ) : null}
            {plan.steps.length === 0 && plan.blocked.length === 0 ? (
              <p className="text-muted-foreground">{t("profiles.apply.nothing")}</p>
            ) : null}
            <Changes title={t("profiles.apply.on", { count: turnOn.length })} steps={turnOn} />
            <Changes title={t("profiles.apply.off", { count: turnOff.length })} steps={turnOff} />
            {noBackup > 0 ? (
              <p className="text-xs text-warning">
                {t("profiles.apply.nobackup", { count: noBackup })}
              </p>
            ) : null}
            {plan.blocked.length > 0 ? (
              <div className="flex flex-col items-start gap-2">
                <p className="text-xs text-warning">
                  {t("profiles.apply.blocked", { count: plan.blocked.length })}
                </p>
                {!elevated ? <ElevateButton route="profiles" /> : null}
              </div>
            ) : null}
            {plan.unknown.length > 0 ? (
              <p className="text-xs text-muted-foreground">
                {t("profiles.apply.unknown", { count: plan.unknown.length })}
              </p>
            ) : null}
            {plan.unavailable.length > 0 ? (
              <p className="text-xs text-muted-foreground">
                {t("profiles.apply.unavailable", { count: plan.unavailable.length })}
              </p>
            ) : null}
            {plan.already > 0 ? (
              <p className="text-xs text-muted-foreground">
                {t("profiles.apply.already", { count: plan.already })}
              </p>
            ) : null}
            {plan.steps.length > 0 ? (
              <p className="text-xs text-muted-foreground">
                {restoreBefore && elevated ? `${t("profiles.apply.restore")} ` : ""}
                {t("profiles.apply.safe")}
              </p>
            ) : null}
          </>
        )}
      </div>
    </Modal>
  );
}

function Changes({ title, steps }: { title: string; steps: ProfileStep[] }) {
  const { t } = useI18n();
  if (steps.length === 0) return null;
  return (
    <div className="flex flex-col gap-1.5">
      <p className="text-xs font-semibold text-muted-foreground">{title}</p>
      <ul className="flex max-h-28 flex-wrap gap-1.5 overflow-auto">
        {steps.map((s) => (
          <li key={s.id}>
            <Badge tone="neutral" className={CHIP}>
              {tweakLabel(t, s.id)}
            </Badge>
          </li>
        ))}
      </ul>
    </div>
  );
}
