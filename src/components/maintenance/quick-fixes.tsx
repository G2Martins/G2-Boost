import {
  CircleCheck,
  CircleMinus,
  CircleX,
  Clock,
  type LucideIcon,
  Printer,
  RefreshCw,
  RotateCcw,
  TriangleAlert,
  Volume2,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, HelpHint, Skeleton } from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import {
  backend,
  inTauri,
  type MaintFix,
  type MaintFixOutcome,
  type MaintFixStep,
  type PrintStatus,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const FIXES: { id: MaintFix; icon: LucideIcon }[] = [
  { id: "wu_reset", icon: RotateCcw },
  { id: "print_queue", icon: Printer },
  { id: "audio", icon: Volume2 },
];

/** Win32 codes worth a word; anything else shows the number. */
const WHY: Partial<Record<number, I18nKey>> = {
  5: "maint3.why.5",
  32: "maint3.why.32",
  1053: "maint3.why.1053",
  1058: "maint3.why.1058",
};

type T = ReturnType<typeof useI18n>["t"];

function stepText(s: MaintFixStep, t: T): string {
  const known = WHY[s.code];
  const why = known ? t(known) : t("maint3.why.other", { code: s.code });
  const p = { target: s.target, note: s.note ?? "", n: s.jobs ?? 0, why };
  const base = `maint3.do.${s.action}`;
  const special = s.note ? `${base}.${s.note}` : null;
  if (special && t(special as I18nKey) !== special) return t(special as I18nKey, p);
  return t((s.ok ? base : `${base}.fail`) as I18nKey, p);
}

/** "Por dentro": the call or command a tool really makes, in one mono line. */
export function Inside({ text }: { text: string }) {
  const { t } = useI18n();
  return (
    <p className="rounded-md border border-border bg-background/50 px-2.5 py-1.5 font-mono text-[10.5px] leading-relaxed text-muted-foreground">
      <span className="font-sans font-semibold text-foreground/70">{t("maint3.inside")}: </span>
      {text}
    </p>
  );
}

/** A small section rule: eyebrow, help, hairline. */
export function SectionRule({ title, help }: { title: string; help?: string }) {
  return (
    <div className="flex items-center gap-2 pt-2">
      <h2 className="text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
        {title}
      </h2>
      {help ? <HelpHint text={help} /> : null}
      <span aria-hidden className="h-px flex-1 bg-linear-to-r from-border-strong to-transparent" />
    </div>
  );
}

/** Windows Update reset, print queue, audio: one card each, confirm inline, steps reported. */
export function QuickFixes({
  elevated,
  servicing,
  index,
}: {
  elevated: boolean;
  /** DISM/SFC/a disk job holds the servicing slot (the Windows Update reset needs it too). */
  servicing: boolean;
  index: number;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [history, setHistory] = useState<MaintFixOutcome[]>([]);
  const [busy, setBusy] = useState<MaintFix | null>(null);
  const [confirm, setConfirm] = useState<MaintFix | null>(null);
  const [error, setError] = useState<{ fix: MaintFix; text: string } | null>(null);
  const [print, setPrint] = useState<PrintStatus | null>(null);
  const [printLoading, setPrintLoading] = useState(inTauri);
  const [since] = useState(() => Date.now() / 1000);

  const loadPrint = useCallback(async () => {
    setPrintLoading(true);
    setPrint(await backend.maintPrintStatus());
    setPrintLoading(false);
  }, []);

  useEffect(() => {
    backend.maintFixHistory().then((h) => h && setHistory(h));
    if (inTauri) void loadPrint();
  }, [loadPrint]);

  const run = async (fix: MaintFix) => {
    setConfirm(null);
    setError(null);
    setBusy(fix);
    try {
      const out = await backend.maintFixRun(fix);
      setHistory((h) => [...h, out]);
      toast(
        `${t(`maint3.fix.${fix}.title` as I18nKey)}: ${t(out.ok ? "maint3.fix.ok" : "maint3.fix.partial")}`,
        { tone: out.ok ? "success" : "warning" },
      );
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      const own: Partial<Record<string, I18nKey>> = {
        busy: "maint3.fix.busy",
        spooler_disabled: "maint3.print.spooler.disabled",
        spooler_missing: "maint3.print.spooler.missing",
      };
      const k = own[msg];
      setError({ fix, text: k ? t(k) : errorText(e) });
    } finally {
      setBusy(null);
      if (fix === "print_queue") void loadPrint();
    }
  };

  const noSpooler = print?.spooler === "disabled" || print?.spooler === "missing";

  return (
    <>
      <SectionRule title={t("maint3.section.fixes")} help={t("maint3.section.fixes.hint")} />
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        {FIXES.map(({ id, icon: Icon }, i) => {
          const last = [...history].reverse().find((o) => o.fix === id) ?? null;
          const blocked =
            !inTauri ||
            !elevated ||
            busy !== null ||
            (id === "wu_reset" && servicing) ||
            (id === "print_queue" && noSpooler);
          return (
            <Card
              key={id}
              className={cn(
                "panel-hover animate-rise flex flex-col gap-3",
                busy === id && "border-primary/40",
              )}
              style={rise(index + i)}
            >
              <div className="flex items-start gap-3">
                <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-foreground/5 text-muted-foreground ring-1 ring-foreground/10 ring-inset">
                  <Icon className="size-5" />
                </span>
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="font-display text-[14px] font-semibold tracking-wide">
                      {t(`maint3.fix.${id}.title` as I18nKey)}
                    </h3>
                    <Badge tone={elevated ? "neutral" : "warning"}>
                      {t("common.admin_required")}
                    </Badge>
                  </div>
                  <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                    {t(`maint3.fix.${id}.body` as I18nKey)}
                  </p>
                </div>
              </div>
              <p className="flex gap-1.5 text-[11px] leading-relaxed text-warning/90">
                <TriangleAlert className="mt-px size-3.5 shrink-0" />
                {t(`maint3.fix.${id}.cost` as I18nKey)}
              </p>
              <Inside text={t(`maint3.fix.${id}.inside` as I18nKey)} />
              <p className="inline-flex items-center gap-1.5 text-[11px] text-muted-foreground">
                <Clock className="size-3" />
                {t(`maint3.fix.${id}.time` as I18nKey)}
              </p>

              {id === "print_queue" ? (
                <PrintPreview status={print} loading={printLoading} onRefresh={loadPrint} />
              ) : null}

              <div className="mt-auto flex items-center gap-2 border-t border-border pt-3 text-xs">
                <span className="text-muted-foreground">{t("maintenance.last")}</span>
                {last ? (
                  <>
                    <Badge tone={last.ok ? "success" : "warning"}>
                      {t(last.ok ? "maint3.fix.ok" : "maint3.fix.partial")}
                    </Badge>
                    <span className="num ml-auto text-muted-foreground">
                      {new Date(last.at * 1000).toLocaleString(locale)}
                    </span>
                  </>
                ) : (
                  <span className="text-muted-foreground/70">{t("maintenance.never")}</span>
                )}
              </div>
              {last && last.steps.length > 0 ? (
                <details open={last.at >= since} className="text-xs">
                  <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
                    {t("maint3.fix.steps", { n: last.steps.length })}
                  </summary>
                  <ul className="mt-2 space-y-1">
                    {last.steps.map((s, k) => {
                      const skipped =
                        s.ok &&
                        (s.note === "missing" || s.note === "disabled" || s.note === "empty");
                      const StepIcon = !s.ok ? CircleX : skipped ? CircleMinus : CircleCheck;
                      return (
                        // biome-ignore lint/suspicious/noArrayIndexKey: a fixed log, never reordered (the same service can appear twice)
                        <li key={k} className="flex items-start gap-2 leading-relaxed">
                          <StepIcon
                            className={cn(
                              "mt-0.5 size-3.5 shrink-0",
                              !s.ok
                                ? "text-destructive"
                                : skipped
                                  ? "text-muted-foreground"
                                  : "text-success",
                            )}
                          />
                          <span className="min-w-0 wrap-break-word text-foreground/85">
                            {stepText(s, t)}
                          </span>
                        </li>
                      );
                    })}
                  </ul>
                </details>
              ) : null}

              {busy === id ? (
                <span
                  className="relative block h-1.5 overflow-hidden rounded-full bg-border"
                  role="progressbar"
                  aria-label={t(`maint3.fix.${id}.title` as I18nKey)}
                >
                  <span className="absolute inset-y-0 w-1/3 rounded-full bg-primary/70 motion-safe:animate-sweep" />
                </span>
              ) : confirm === id ? (
                <div className="flex flex-wrap items-center gap-2 rounded-md bg-warning/8 p-3 ring-1 ring-warning/25 ring-inset">
                  <p className="text-xs text-foreground/90">
                    {t(`maint3.fix.${id}.confirm` as I18nKey)}
                  </p>
                  <div className="ml-auto flex gap-2">
                    <Button variant="ghost" size="sm" onClick={() => setConfirm(null)}>
                      {t("common.cancel")}
                    </Button>
                    <Button variant="primary" size="sm" onClick={() => void run(id)}>
                      {t(`maint3.fix.${id}.button` as I18nKey)}
                    </Button>
                  </div>
                </div>
              ) : (
                <div>
                  <Button
                    variant="secondary"
                    size="sm"
                    disabled={blocked}
                    onClick={() => setConfirm(id)}
                  >
                    {t(`maint3.fix.${id}.button` as I18nKey)}
                  </Button>
                </div>
              )}
              {error?.fix === id ? <p className="text-xs text-destructive">{error.text}</p> : null}
            </Card>
          );
        })}
      </div>
    </>
  );
}

/** The local queues and their jobs before anything is cancelled. */
function PrintPreview({
  status,
  loading,
  onRefresh,
}: {
  status: PrintStatus | null;
  loading: boolean;
  onRefresh: () => void;
}) {
  const { t } = useI18n();
  if (!inTauri) return <p className="text-[11px] text-muted-foreground">{t("common.app_only")}</p>;
  if (loading && !status) return <Skeleton className="h-12 w-full" />;
  if (status?.spooler !== "running") {
    const s = status?.spooler ?? "unknown";
    return (
      <p
        className={cn(
          "text-[11px] leading-relaxed",
          s === "stopped" ? "text-muted-foreground" : "text-warning",
        )}
      >
        {t(`maint3.print.spooler.${s}` as I18nKey)}
      </p>
    );
  }
  const printers = status.printers ?? [];
  const queued = printers.reduce((n, p) => n + p.jobs, 0);
  return (
    <div className="rounded-md border border-border bg-background/40">
      {printers.length === 0 ? (
        <p className="px-2.5 py-2 text-[11px] text-muted-foreground">{t("maint3.print.none")}</p>
      ) : (
        <ul className="max-h-40 divide-y divide-border/60 overflow-auto">
          {printers.map((p) => (
            <li key={p.name} className="flex items-center gap-2 px-2.5 py-1.5 text-[12px]">
              <span className="min-w-0 flex-1 truncate" title={p.name}>
                {p.name}
              </span>
              <Badge tone={p.jobs > 0 ? "warning" : "neutral"}>
                {t("maint3.print.jobs", { n: p.jobs })}
              </Badge>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-center gap-2 border-t border-border px-2.5 py-1.5 text-[11px] text-muted-foreground">
        <span className="min-w-0 flex-1">{queued === 0 ? t("maint3.print.idle") : null}</span>
        <button
          type="button"
          onClick={onRefresh}
          className="press inline-flex items-center gap-1 hover:text-foreground"
        >
          <RefreshCw className={cn("size-3", loading && "motion-safe:animate-spin")} />
          {t("maint3.print.refresh")}
        </button>
      </div>
    </div>
  );
}
