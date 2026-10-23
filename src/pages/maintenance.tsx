import {
  Clock,
  FileCheck2,
  History,
  type LucideIcon,
  ScanSearch,
  ShieldCheck,
  Square,
  Wrench,
} from "lucide-react";
import { useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, EmptyState, HelpHint } from "@/components/ui/primitives";
import {
  backend,
  inTauri,
  type RepairOutcome,
  type RepairProgress,
  type RepairStatus,
  type RepairTool,
  type RepairVerdict,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

type CardKey = "dism_scan" | "dism_restore" | "sfc" | "mrt";

/** Recommended order first (image → repair → files); the malware scan stands apart. */
const CARDS: { key: CardKey; step: number | null; icon: LucideIcon; tools: RepairTool[] }[] = [
  { key: "dism_scan", step: 1, icon: ScanSearch, tools: ["dism_scan"] },
  { key: "dism_restore", step: 2, icon: Wrench, tools: ["dism_restore"] },
  { key: "sfc", step: 3, icon: FileCheck2, tools: ["sfc"] },
  { key: "mrt", step: null, icon: ShieldCheck, tools: ["mrt_quick", "mrt_full"] },
];

const TIME_KEY: Record<RepairTool, I18nKey> = {
  dism_scan: "maintenance.dism_scan.time",
  dism_restore: "maintenance.dism_restore.time",
  sfc: "maintenance.sfc.time",
  mrt_quick: "maintenance.mrt_quick.time",
  mrt_full: "maintenance.mrt_full.time",
};

const TONE: Record<RepairVerdict, "success" | "warning" | "danger" | "neutral"> = {
  healthy: "success",
  clean: "success",
  repaired: "success",
  repairable: "warning",
  threats: "danger",
  non_repairable: "danger",
  failed: "danger",
  cancelled: "neutral",
  unknown: "neutral",
};

/** Codes worth a sentence of their own; anything else shows the raw code. */
function codeKey(code: number): I18nKey | null {
  if (code === 1460) return "maintenance.code.timeout";
  if ([0x800f081f, 0x800f0906, 0x800f0907, 0x800f0950].includes(code))
    return "maintenance.code.source";
  if (code === 0x800f082f) return "maintenance.code.pending";
  return null;
}

const hex = (code: number) => `0x${code.toString(16).toUpperCase().padStart(8, "0")}`;

function elapsed(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

export function MaintenancePage() {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [elevated, setElevated] = useState(false);
  const [status, setStatus] = useState<RepairStatus | null>(null);
  const [history, setHistory] = useState<RepairOutcome[]>([]);
  const [confirm, setConfirm] = useState<RepairTool | null>(null);
  const [run, setRun] = useState<{
    tool: RepairTool;
    percent: number | null;
    since: number;
    /** Started before this page mounted: nobody awaits it here, so poll for the end. */
    adopted?: boolean;
  } | null>(null);
  const [error, setError] = useState<{ card: CardKey; text: string } | null>(null);
  const [, tick] = useState(0);

  useEffect(() => {
    backend.appInfo().then((i) => i && setElevated(i.elevated));
    backend.repairHistory().then((h) => h && setHistory(h));
    backend.repairStatus().then((s) => {
      if (!s) return;
      setStatus(s);
      // Came back to the page mid-run: keep showing it (start time unknown → from now).
      if (s.running)
        setRun({ tool: s.running, percent: s.percent, since: Date.now(), adopted: true });
    });
    let off: (() => void) | null = null;
    backend
      .on<RepairProgress>("maintenance:progress", (p) =>
        setRun((r) => (r && r.tool === p.tool ? { ...r, percent: p.percent } : r)),
      )
      .then((u) => {
        off = u;
      });
    return () => off?.();
  }, []);

  useEffect(() => {
    if (!run) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [run]);

  const adopted = run?.adopted === true;
  useEffect(() => {
    if (!adopted) return;
    const id = window.setInterval(async () => {
      const s = await backend.repairStatus();
      if (s && !s.running) {
        setRun(null);
        const h = await backend.repairHistory();
        if (h) setHistory(h);
      }
    }, 3000);
    return () => window.clearInterval(id);
  }, [adopted]);

  const start = async (tool: RepairTool, card: CardKey) => {
    setConfirm(null);
    setError(null);
    setRun({ tool, percent: null, since: Date.now() });
    try {
      await backend.repairRun(tool);
      const h = await backend.repairHistory();
      if (h) setHistory(h);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      const text =
        msg === "busy"
          ? t("maintenance.busy")
          : msg === "mrt_missing"
            ? t("maintenance.mrt.missing")
            : errorText(e);
      setError({ card, text });
    } finally {
      setRun(null);
    }
  };

  const last = (tools: RepairTool[]) =>
    [...history].reverse().find((o) => tools.includes(o.tool)) ?? null;

  const detail = (o: RepairOutcome): string | null => {
    if (o.verdict === "cancelled") return null;
    if (o.verdict === "failed") {
      const k = codeKey(o.code);
      return k ? t(k) : t("maintenance.code.other", { code: hex(o.code) });
    }
    if (o.tool === "dism_scan" && o.verdict === "repairable")
      return t("maintenance.detail.repairable");
    if (o.tool.startsWith("dism") && o.verdict === "non_repairable")
      return t("maintenance.detail.dism_bad");
    if (o.tool === "sfc" && (o.repaired > 0 || o.unrepaired > 0))
      return t("maintenance.detail.sfc", { repaired: o.repaired, unrepaired: o.unrepaired });
    if (o.verdict === "unknown") return t("maintenance.detail.unknown");
    if (o.threats.length > 0)
      return t("maintenance.detail.threats", { names: o.threats.join(", ") });
    return null;
  };

  const blocked = !inTauri || !elevated || run !== null;
  const mrtMissing = status !== null && !status.mrt_present;

  return (
    <>
      <PageHeader title={t("maintenance.title")} subtitle={t("maintenance.subtitle")} />
      <PageBody>
        {!elevated ? (
          <Card className="animate-rise flex flex-wrap items-center gap-4 border-warning/40 py-4">
            <span className="flex flex-col gap-0.5">
              <span className="font-display text-[15px] font-semibold tracking-wide">
                {t("maintenance.admin.title")}
              </span>
              <span className="text-xs text-muted-foreground">{t("maintenance.admin.body")}</span>
            </span>
            <span className="ml-auto">
              <ElevateButton route="maintenance" size="md" />
            </span>
          </Card>
        ) : null}

        {/* The recommended order as a rail: each step feeds the next. */}
        <Card className="animate-rise py-4" style={rise(1)}>
          <div className="flex items-center gap-2 text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
            {t("maintenance.order.title")}
            <HelpHint text={t("maintenance.order.hint")} />
          </div>
          <ol className="mt-3 grid grid-cols-1 gap-3 md:grid-cols-3">
            {[1, 2, 3].map((n) => (
              <li key={n} className="relative flex items-center gap-3">
                <span className="num flex size-7 shrink-0 items-center justify-center rounded-full font-display text-[13px] font-semibold ring-1 ring-border-strong ring-inset">
                  {n}
                </span>
                <span className="text-[13px]">{t(`maintenance.order.${n}` as I18nKey)}</span>
                {n < 3 ? (
                  <span
                    aria-hidden
                    className="ml-auto hidden h-px flex-1 bg-linear-to-r from-border-strong to-transparent md:block"
                  />
                ) : null}
              </li>
            ))}
          </ol>
        </Card>

        <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
          {CARDS.map((c, i) => {
            const prev = last(c.tools);
            const mine = run && c.tools.includes(run.tool) ? run : null;
            const pending = confirm && c.tools.includes(confirm) ? confirm : null;
            const Icon = c.icon;
            const missing = c.key === "mrt" && mrtMissing;
            return (
              <Card
                key={c.key}
                className={cn(
                  "panel-hover animate-rise flex flex-col gap-3",
                  mine && "border-primary/40",
                )}
                style={rise(i + 2)}
              >
                <div className="flex items-start gap-3">
                  <span className="relative flex size-10 shrink-0 items-center justify-center rounded-lg bg-foreground/5 text-muted-foreground ring-1 ring-foreground/10 ring-inset">
                    <Icon className="size-5" />
                    {c.step ? (
                      <span className="num absolute -top-1.5 -left-1.5 flex size-4.5 items-center justify-center rounded-full bg-surface font-display text-[10px] font-semibold text-foreground ring-1 ring-border-strong">
                        {c.step}
                      </span>
                    ) : null}
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <h3 className="font-display text-[14px] font-semibold tracking-wide">
                        {t(`maintenance.${c.key}.title` as I18nKey)}
                      </h3>
                      <Badge tone={elevated ? "neutral" : "warning"}>
                        {t("common.admin_required")}
                      </Badge>
                    </div>
                    <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
                      {t(`maintenance.${c.key}.body` as I18nKey)}
                    </p>
                    {c.key === "mrt" ? (
                      <p className="mt-1.5 text-[11px] text-warning/90">
                        {t("maintenance.mrt.not_av")}
                      </p>
                    ) : null}
                  </div>
                </div>

                <div className="flex flex-wrap items-center gap-x-4 gap-y-1 text-[11px] text-muted-foreground">
                  {c.tools.map((tool) => (
                    <span key={tool} className="inline-flex items-center gap-1.5">
                      <Clock className="size-3" />
                      {c.tools.length > 1 ? `${t(`maintenance.${tool}` as I18nKey)}: ` : ""}
                      {t(TIME_KEY[tool])}
                    </span>
                  ))}
                </div>

                <div className="flex items-center gap-2 border-t border-border pt-3 text-xs">
                  <span className="text-muted-foreground">{t("maintenance.last")}</span>
                  {prev ? (
                    <>
                      <Badge tone={TONE[prev.verdict]}>
                        {t(`maintenance.verdict.${prev.verdict}` as I18nKey)}
                      </Badge>
                      <span className="num ml-auto text-muted-foreground">
                        {new Date(prev.at * 1000).toLocaleString(locale)}
                      </span>
                    </>
                  ) : (
                    <span className="text-muted-foreground/70">{t("maintenance.never")}</span>
                  )}
                </div>
                {prev && detail(prev) ? (
                  <p className="-mt-1.5 text-xs text-foreground/85">{detail(prev)}</p>
                ) : null}

                {mine ? (
                  <div className="flex items-center gap-3">
                    <span
                      className="relative block h-1.5 flex-1 overflow-hidden rounded-full bg-border"
                      role="progressbar"
                      aria-valuemin={0}
                      aria-valuemax={100}
                      aria-valuenow={mine.percent ?? undefined}
                    >
                      {mine.percent !== null ? (
                        <span
                          className="block h-full rounded-full bg-primary transition-[width] duration-300"
                          style={{ width: `${mine.percent}%` }}
                        />
                      ) : (
                        <span className="absolute inset-y-0 w-1/3 animate-sweep rounded-full bg-primary/70" />
                      )}
                    </span>
                    <span className="num w-20 text-right text-xs text-muted-foreground">
                      {mine.percent !== null ? `${mine.percent}% · ` : ""}
                      {elapsed(Date.now() - mine.since)}
                    </span>
                    <Button variant="ghost" size="sm" onClick={() => void backend.repairCancel()}>
                      <Square className="size-3.5" />
                      {t("common.stop")}
                    </Button>
                  </div>
                ) : pending ? (
                  <div className="flex flex-wrap items-center gap-2 rounded-md bg-warning/8 p-3 ring-1 ring-warning/25 ring-inset">
                    <p className="text-xs text-foreground/90">
                      {t("maintenance.confirm", { time: t(TIME_KEY[pending]) })}
                    </p>
                    <div className="ml-auto flex gap-2">
                      <Button variant="ghost" size="sm" onClick={() => setConfirm(null)}>
                        {t("common.cancel")}
                      </Button>
                      <Button variant="primary" size="sm" onClick={() => start(pending, c.key)}>
                        {t("maintenance.confirm.go")}
                      </Button>
                    </div>
                  </div>
                ) : missing ? (
                  <p className="text-xs text-muted-foreground">{t("maintenance.mrt.missing")}</p>
                ) : (
                  <div className="flex flex-wrap gap-2">
                    {c.tools.map((tool) => (
                      <Button
                        key={tool}
                        variant={c.tools.length > 1 && tool === "mrt_full" ? "outline" : "primary"}
                        size="sm"
                        disabled={blocked}
                        onClick={() => setConfirm(tool)}
                      >
                        {c.tools.length > 1
                          ? t(`maintenance.${tool}` as I18nKey)
                          : t("maintenance.start")}
                      </Button>
                    ))}
                  </div>
                )}
                {error?.card === c.key ? (
                  <p className="text-xs text-destructive">{error.text}</p>
                ) : null}
              </Card>
            );
          })}
        </div>

        <Card className="animate-rise" style={rise(CARDS.length + 2)}>
          <CardTitle>
            <History className="size-4" />
            {t("maintenance.history")}
          </CardTitle>
          {history.length === 0 ? (
            <EmptyState
              icon={<History className="size-5" />}
              title={t("maintenance.history.empty")}
              body={t("maintenance.order.hint")}
            />
          ) : (
            <ul className="divide-y divide-border text-xs">
              {[...history]
                .reverse()
                .slice(0, 10)
                .map((o) => (
                  <li key={`${o.at}-${o.tool}`} className="flex items-center gap-3 py-2">
                    <span className="num text-muted-foreground">
                      {new Date(o.at * 1000).toLocaleString(locale)}
                    </span>
                    <span>{t(`maintenance.tool.${o.tool}` as I18nKey)}</span>
                    <Badge tone={TONE[o.verdict]} className="ml-auto">
                      {t(`maintenance.verdict.${o.verdict}` as I18nKey)}
                    </Badge>
                    <span className="num w-12 text-right text-muted-foreground">
                      {elapsed(o.duration_ms)}
                    </span>
                  </li>
                ))}
            </ul>
          )}
        </Card>
      </PageBody>
    </>
  );
}
