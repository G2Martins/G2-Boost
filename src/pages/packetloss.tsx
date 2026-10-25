import { FileDown, Loader2, Play, Route as RouteIcon, Square, Waypoints } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { PacketChart } from "@/components/tools/packet-chart";
import { Stat } from "@/components/tools/stat";
import { Button, Card, CardTitle, EmptyState, Meter, StatusPill } from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import {
  backend,
  inTauri,
  type PacketLossEvent,
  type PacketLossProfile,
  type PacketLossReport,
  type PacketLossSummary,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import {
  type Cell,
  type CellState,
  cellState,
  packetCount,
  type RouteVerdict,
  routeVerdict,
  type TargetKind,
  verdict,
} from "@/lib/pktloss";
import { analyzeRoute, type RouteAnalysis, type Zone } from "@/lib/route";
import { cn } from "@/lib/utils";

type Choice = "gateway" | "cloudflare" | "google" | "custom";
const FIXED = { cloudflare: "1.1.1.1", google: "8.8.8.8" } as const;

const PROFILE_TEXT: Record<PacketLossProfile, { name: I18nKey; hint: I18nKey }> = {
  quick: { name: "tools.loss.profile.quick", hint: "tools.loss.profile.quick.hint" },
  standard: { name: "tools.loss.profile.standard", hint: "tools.loss.profile.standard.hint" },
  long: { name: "tools.loss.profile.long", hint: "tools.loss.profile.long.hint" },
};

/** Core error codes (`pktloss::TargetError` and `RunError`) → text. */
const RUN_ERRORS: Record<string, I18nKey> = {
  busy: "tools.loss.error.busy",
  empty: "tools.loss.error.empty",
  too_long: "tools.loss.error.too_long",
  invalid: "tools.loss.error.invalid",
  not_allowed: "tools.loss.error.not_allowed",
  resolve: "tools.loss.error.resolve",
};

const CSV_ERRORS: Record<string, I18nKey> = {
  no_run: "tools.loss.csv.error.no_run",
  no_folder: "tools.loss.csv.error.no_folder",
  io: "tools.loss.csv.error.io",
};

const ZONE_TEXT: Record<Zone, { where: I18nKey; name: I18nKey }> = {
  home: { where: "tools.loss.route.where.home", name: "tools.loss.route.zone.home" },
  isp: { where: "tools.loss.route.where.isp", name: "tools.loss.route.zone.isp" },
  internet: { where: "tools.loss.route.where.internet", name: "tools.loss.route.zone.internet" },
};

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** The strip of the finished run: every packet that got an outcome, by sequence number. */
function cellsOf(r: PacketLossReport, total: number): (Cell | undefined)[] {
  const out: (Cell | undefined)[] = Array.from({ length: total }, () => undefined);
  for (const s of r.samples) {
    if (s.seq < total) out[s.seq] = { rtt: s.rtt_ms, late: s.late, ooo: s.out_of_order };
  }
  return out;
}

type RouteResult = { analysis: RouteAnalysis; verdict: RouteVerdict };

export function PacketLossPage() {
  const { t, locale } = useI18n();
  const [choice, setChoice] = useState<Choice>("cloudflare");
  const [custom, setCustom] = useState("");
  const [profile, setProfile] = useState<PacketLossProfile>("standard");
  // undefined: still looking; null: no router found (or outside the app).
  const [gateway, setGateway] = useState<string | null | undefined>(undefined);
  const [phase, setPhase] = useState<"idle" | "running" | "done">("idle");
  const [cells, setCells] = useState<(Cell | undefined)[]>([]);
  const [live, setLive] = useState<PacketLossSummary | null>(null);
  const [report, setReport] = useState<PacketLossReport | null>(null);
  const [kind, setKind] = useState<TargetKind>("internet");
  const [error, setError] = useState<string | null>(null);
  const [route, setRoute] = useState<RouteResult | null>(null);
  const [routeBusy, setRouteBusy] = useState(false);
  const [routeError, setRouteError] = useState<string | null>(null);
  const running = useRef(false);

  useEffect(() => {
    let alive = true;
    backend.dnsAdapters().then((a) => {
      if (!alive) return;
      const pick = a?.find((x) => x.primary && x.gateway) ?? a?.find((x) => x.gateway);
      setGateway(pick?.gateway ?? null);
    });
    return () => {
      alive = false;
    };
  }, []);

  // Leaving the page ends the run: nobody is watching it.
  useEffect(
    () => () => {
      if (running.current) void backend.packetlossCancel();
    },
    [],
  );

  const target =
    choice === "gateway" ? gateway : choice === "custom" ? custom.trim() : FIXED[choice];
  const busy = phase === "running";
  const summary = report?.summary ?? live;
  const total = cells.length;
  const done = cells.reduce((n, c) => n + (c ? 1 : 0), 0);

  const start = async () => {
    if (!inTauri || !target || running.current) return;
    running.current = true;
    const count = packetCount(profile);
    setError(null);
    setReport(null);
    setRoute(null);
    setRouteError(null);
    setLive(null);
    setCells(Array.from({ length: count }, () => undefined));
    setKind(choice === "gateway" ? "gateway" : choice === "custom" ? "custom" : "internet");
    setPhase("running");
    const off = await backend.on<PacketLossEvent>("packetloss:probe", (ev) => {
      setCells((prev) => {
        const next = prev.slice();
        next[ev.seq] = { rtt: ev.rtt_ms, late: ev.late, ooo: ev.out_of_order };
        return next;
      });
      setLive(ev.summary);
    });
    try {
      const r = await backend.packetlossRun(target, profile);
      setReport(r);
      setCells(cellsOf(r, count));
      setPhase("done");
    } catch (e) {
      const code = message(e);
      setError(
        RUN_ERRORS[code] ? t(RUN_ERRORS[code]) : t("tools.loss.error.generic", { error: code }),
      );
      setCells([]);
      setLive(null);
      setPhase("idle");
    } finally {
      off();
      running.current = false;
    }
  };

  const saveCsv = async () => {
    try {
      toast(t("tools.loss.csv.saved", { name: await backend.packetlossExport(locale) }), {
        tone: "success",
      });
    } catch (e) {
      const code = message(e);
      toast(
        CSV_ERRORS[code] ? t(CSV_ERRORS[code]) : t("tools.loss.error.generic", { error: code }),
        {
          tone: "danger",
        },
      );
    }
  };

  const analyze = async () => {
    if (!report || routeBusy) return;
    setRouteBusy(true);
    setRoute(null);
    setRouteError(null);
    try {
      // The address the core already validated and resolved, not the text that was typed.
      const hops = await backend.traceHost(report.ip, 30, 3, 1200);
      const analysis = analyzeRoute(hops);
      setRoute({ analysis, verdict: routeVerdict(analysis, report.summary.loss_pct) });
    } catch (e) {
      setRouteError(t("tools.loss.route.error", { error: message(e) }));
    } finally {
      setRouteBusy(false);
    }
  };

  const nf = useMemo(
    () => ({
      ms: (v: number | null) =>
        v === null
          ? "—"
          : new Intl.NumberFormat(locale, { maximumFractionDigits: v < 10 ? 1 : 0 }).format(v),
      pct: (v: number) => new Intl.NumberFormat(locale, { maximumFractionDigits: 1 }).format(v),
      n: (v: number) => new Intl.NumberFormat(locale).format(v),
    }),
    [locale],
  );

  const v = summary ? verdict(summary, kind, phase !== "running") : null;
  const gatewayText =
    gateway === undefined ? "…" : (gateway ?? t("tools.loss.target.gateway.none"));

  return (
    <>
      <PageHeader title={t("nav.packetloss")} subtitle={t("tools.loss.subtitle")} />
      <PageBody>
        <Card className="animate-rise flex flex-col gap-5">
          <CardTitle help={t("tools.loss.help")} className="mb-0">
            {t("tools.loss.target")}
          </CardTitle>
          <div className="flex flex-wrap items-stretch gap-2">
            <Choose
              active={choice === "gateway"}
              disabled={busy || !gateway}
              onClick={() => setChoice("gateway")}
              title={t("tools.loss.target.gateway")}
              hint={gatewayText}
            />
            <Choose
              active={choice === "cloudflare"}
              disabled={busy}
              onClick={() => setChoice("cloudflare")}
              title={t("tools.loss.target.cloudflare")}
              hint={FIXED.cloudflare}
            />
            <Choose
              active={choice === "google"}
              disabled={busy}
              onClick={() => setChoice("google")}
              title={t("tools.loss.target.google")}
              hint={FIXED.google}
            />
            <Choose
              active={choice === "custom"}
              disabled={busy}
              onClick={() => setChoice("custom")}
              title={t("tools.loss.target.custom")}
            />
            {choice === "custom" ? (
              <label className="flex h-auto min-w-64 flex-1 flex-col justify-center rounded-md border border-border bg-background px-3 py-1.5">
                <span className="text-[10px] text-muted-foreground">
                  {t("tools.loss.custom.label")}
                </span>
                <input
                  className="bg-transparent text-[13px] outline-none"
                  value={custom}
                  onChange={(e) => setCustom(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter") void start();
                  }}
                  placeholder={t("tools.loss.custom.placeholder")}
                  maxLength={253}
                  spellCheck={false}
                  disabled={busy}
                />
              </label>
            ) : null}
          </div>

          <div className="flex flex-wrap items-end justify-between gap-4">
            <div>
              <h3 className="mb-2 font-display text-[15px] font-semibold tracking-wide">
                {t("tools.loss.profile")}
              </h3>
              <div className="flex flex-wrap gap-2">
                {(Object.keys(PROFILE_TEXT) as PacketLossProfile[]).map((p) => (
                  <Choose
                    key={p}
                    active={profile === p}
                    disabled={busy}
                    onClick={() => setProfile(p)}
                    title={t(PROFILE_TEXT[p].name)}
                    hint={t(PROFILE_TEXT[p].hint)}
                  />
                ))}
              </div>
            </div>
            <div className="flex flex-col items-end gap-2">
              {busy ? (
                <Button variant="outline" size="lg" onClick={() => void backend.packetlossCancel()}>
                  <Square className="size-4" />
                  {t("tools.loss.stop")}
                </Button>
              ) : (
                <Button
                  variant="primary"
                  tactile
                  size="lg"
                  onClick={start}
                  disabled={!inTauri || !target}
                >
                  <Play className="size-4" />
                  {t("tools.loss.start")}
                </Button>
              )}
              {!inTauri ? (
                <span className="text-xs text-muted-foreground">{t("common.app_only")}</span>
              ) : null}
            </div>
          </div>
          {error ? (
            <p role="alert" className="text-xs text-destructive">
              {error}
            </p>
          ) : null}
        </Card>

        {summary && v ? (
          <>
            <Card className="animate-rise frame-ticks" style={rise(1)}>
              <CardTitle
                right={
                  <span className="flex items-center gap-2">
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={saveCsv}
                      disabled={busy || !report || report.samples.length === 0}
                    >
                      <FileDown className="size-4" />
                      {t("tools.loss.csv")}
                    </Button>
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={analyze}
                      disabled={busy || !report || routeBusy}
                    >
                      {routeBusy ? (
                        <Loader2 className="size-4 animate-spin" />
                      ) : (
                        <RouteIcon className="size-4" />
                      )}
                      {t("tools.loss.route.button")}
                    </Button>
                  </span>
                }
              >
                {report?.host ?? target}
              </CardTitle>
              <div className="flex flex-wrap items-start justify-between gap-6">
                <div className="min-w-0 max-w-2xl">
                  <StatusPill tone={v.tone}>{t(v.title)}</StatusPill>
                  <p className="mt-3 text-[13px] leading-relaxed" data-testid="loss-verdict">
                    {t(v.body, {
                      lost: summary.lost,
                      sent: summary.sent,
                      pct: nf.pct(summary.loss_pct),
                      run: summary.longest_loss_run,
                      late: summary.late,
                      ooo: summary.out_of_order,
                      jitter: nf.ms(summary.jitter_ms),
                      avg: nf.ms(summary.avg_ms),
                    })}
                  </p>
                  {v.advice ? (
                    <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
                      {t(v.advice)}
                    </p>
                  ) : null}
                </div>
                <Stat
                  label={t("tools.loss.loss")}
                  value={`${nf.pct(summary.loss_pct)}%`}
                  tone={summary.lost === 0 ? "success" : v.tone === "danger" ? "danger" : "warning"}
                  size="lg"
                  className="text-right"
                />
              </div>
              {busy ? (
                <div className="mt-4 flex items-center gap-3 text-xs text-muted-foreground">
                  <Meter
                    value={total ? done / total : 0}
                    className="flex-1"
                    label={t("tools.loss.progress", { done, total })}
                  />
                  <span className="num">{t("tools.loss.progress", { done, total })}</span>
                </div>
              ) : null}
              <div className="mt-5 grid grid-cols-2 gap-x-6 gap-y-4 border-t border-border pt-4 sm:grid-cols-3 lg:grid-cols-5">
                <Stat
                  label={t("tools.loss.lost")}
                  value={nf.n(summary.lost)}
                  tone={summary.lost > 0 ? "danger" : undefined}
                />
                <Stat
                  label={t("tools.loss.late")}
                  value={nf.n(summary.late)}
                  tone={summary.late > 0 ? "warning" : undefined}
                />
                <Stat
                  label={t("tools.loss.ooo")}
                  value={nf.n(summary.out_of_order)}
                  tone={summary.out_of_order > 0 ? "warning" : undefined}
                />
                <Stat label={t("tools.loss.run")} value={nf.n(summary.longest_loss_run)} />
                <Stat label={t("tools.loss.jitter")} value={nf.ms(summary.jitter_ms)} unit="ms" />
                <Stat label={t("tools.loss.avg")} value={nf.ms(summary.avg_ms)} unit="ms" />
                <Stat label={t("tools.loss.min")} value={nf.ms(summary.min_ms)} unit="ms" />
                <Stat label={t("tools.loss.max")} value={nf.ms(summary.max_ms)} unit="ms" />
                <Stat
                  label={t("tools.loss.p95")}
                  value={nf.ms(summary.p95_ms)}
                  unit="ms"
                  help={t("tools.loss.p95.help")}
                />
              </div>
            </Card>

            <Card className="animate-rise" style={rise(2)}>
              <CardTitle
                right={
                  <span className="flex flex-wrap items-center gap-x-4 gap-y-1 text-xs text-muted-foreground">
                    <Legend cls={CELL_CLASS.ok} label={t("tools.loss.legend.ok")} />
                    <Legend cls={CELL_CLASS.late} label={t("tools.loss.legend.late")} />
                    <Legend cls={CELL_CLASS.lost} label={t("tools.loss.legend.lost")} />
                    <Legend
                      cls={cn(CELL_CLASS.ok, "ring-1 ring-foreground/80")}
                      label={t("tools.loss.legend.ooo")}
                    />
                    <Legend cls={CELL_CLASS.pending} label={t("tools.loss.legend.pending")} />
                  </span>
                }
              >
                {t("tools.loss.strip")}
              </CardTitle>
              <Strip cells={cells} />
              <div className="mt-5">
                <p className="mb-1 text-[10px] font-semibold uppercase tracking-[0.12em] text-muted-foreground">
                  {t("tools.loss.chart")}
                </p>
                <PacketChart
                  cells={cells}
                  label={t("tools.loss.chart.label")}
                  limitLabel={t("tools.loss.chart.limit")}
                />
              </div>
            </Card>
          </>
        ) : (
          <Card className="animate-rise" style={rise(1)}>
            <EmptyState
              icon={
                busy ? (
                  <Loader2 className="size-5 animate-spin text-primary" />
                ) : (
                  <Waypoints className="size-5" />
                )
              }
              title={busy ? t("tools.loss.verdict.collecting") : t("tools.loss.empty.title")}
              body={busy ? undefined : t("tools.loss.empty.body")}
            />
          </Card>
        )}

        {routeBusy || route || routeError ? (
          <Card className="animate-rise" style={rise(3)}>
            <CardTitle>{t("tools.loss.route.title")}</CardTitle>
            {routeBusy ? (
              <p className="flex items-center gap-2 text-[13px] text-muted-foreground">
                <Loader2 className="size-4 animate-spin" />
                {t("tools.loss.route.busy")}
              </p>
            ) : routeError ? (
              <p role="alert" className="text-xs text-destructive">
                {routeError}
              </p>
            ) : route ? (
              <RouteBlock route={route} />
            ) : null}
          </Card>
        ) : null}
      </PageBody>
    </>
  );
}

/* ---------------- pieces ---------------- */

/** A choice in a row of choices: pressed = chosen. */
function Choose({
  active,
  disabled,
  onClick,
  title,
  hint,
}: {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  title: string;
  hint?: string;
}) {
  return (
    <button
      type="button"
      aria-pressed={active}
      disabled={disabled}
      onClick={onClick}
      className={cn(
        "press flex min-w-28 flex-col items-start rounded-md border px-3 py-2 text-left transition-colors duration-200 disabled:opacity-40",
        active
          ? "border-ring bg-card-hover"
          : "border-border bg-background text-muted-foreground hover:text-foreground",
      )}
    >
      <span className="font-display text-[13px] font-semibold">{title}</span>
      {hint ? <span className="num text-[11px] text-muted-foreground">{hint}</span> : null}
    </button>
  );
}

const CELL_CLASS = {
  pending: "bg-foreground/10",
  ok: "bg-success/70",
  late: "bg-warning",
  lost: "bg-destructive",
} as const;

function Legend({ cls, label }: { cls: string; label: string }) {
  return (
    <span className="flex items-center gap-1.5">
      <span aria-hidden className={cn("size-2.5 rounded-[2px]", cls)} />
      {label}
    </span>
  );
}

/** One square per packet, in the order they were sent. */
function Strip({ cells }: { cells: (Cell | undefined)[] }) {
  const { t } = useI18n();
  const items = useMemo(() => cells.map((cell, seq) => ({ seq, cell })), [cells]);
  const count = (s: CellState) =>
    items.reduce((n, it) => n + (cellState(it.cell) === s ? 1 : 0), 0);
  const text = (seq: number, cell: Cell | undefined) => {
    const state = cellState(cell);
    const base = t(`tools.loss.cell.${state}`, { seq: seq + 1, ms: cell?.rtt ?? 0 });
    return cell?.ooo ? `${base}, ${t("tools.loss.cell.ooo")}` : base;
  };
  return (
    <div
      role="img"
      aria-label={t("tools.loss.strip.label", {
        total: items.length,
        ok: count("ok"),
        late: count("late"),
        lost: count("lost"),
      })}
      className="flex flex-wrap gap-[3px]"
      data-testid="loss-strip"
    >
      {items.map(({ seq, cell }) => (
        <span
          key={seq}
          title={text(seq, cell)}
          className={cn(
            "size-2.5 rounded-[2px]",
            CELL_CLASS[cellState(cell)],
            cell?.ooo && "ring-1 ring-foreground/80",
          )}
        />
      ))}
    </div>
  );
}

function RouteBlock({ route }: { route: RouteResult }) {
  const { t, locale } = useI18n();
  const { analysis, verdict: rv } = route;
  const ms = (v: number | null) =>
    v === null
      ? "—"
      : new Intl.NumberFormat(locale, { maximumFractionDigits: v < 10 ? 1 : 0 }).format(v);
  const head =
    rv.kind === "real"
      ? {
          tone: "danger" as const,
          title: t("tools.loss.route.real"),
          body: t("tools.loss.route.real.body", {
            ttl: rv.hop.ttl,
            addr: rv.hop.addr ?? "?",
            where: t(ZONE_TEXT[rv.hop.zone].where),
          }),
        }
      : rv.kind === "unseen"
        ? {
            tone: "warning" as const,
            title: t("tools.loss.route.unseen"),
            body: t("tools.loss.route.unseen.body"),
          }
        : rv.kind === "ignores_ping"
          ? {
              tone: "success" as const,
              title: t("tools.loss.route.ignores"),
              body: t("tools.loss.route.ignores.body", { hops: rv.hops }),
            }
          : {
              tone: "success" as const,
              title: t("tools.loss.route.clean"),
              body: t("tools.loss.route.clean.body"),
            };
  const from = analysis.lossFrom;
  const th = (label: string, right = false) => (
    <th className={cn("py-2 font-medium", right ? "text-right" : "text-left")}>{label}</th>
  );
  return (
    <div className="space-y-4">
      <div>
        <StatusPill tone={head.tone}>{head.title}</StatusPill>
        <p className="mt-3 max-w-3xl text-[13px] leading-relaxed" data-testid="route-verdict">
          {head.body}
        </p>
      </div>
      <table className="w-full text-xs">
        <thead className="text-muted-foreground">
          <tr className="border-b border-border">
            {th(t("network.tools.hop"))}
            {th(t("network.tools.from"))}
            {th(t("tools.loss.route.zone"))}
            {th(t("network.tools.loss"), true)}
            {th(t("network.avg"), true)}
          </tr>
        </thead>
        <tbody>
          {analysis.nodes.map((n, i) => (
            <tr
              key={n.ttl}
              className={cn(
                "border-b border-border/60 last:border-0",
                from !== null && i >= from && n.avg !== null && "bg-destructive/8",
              )}
            >
              <td className="num py-1.5">{n.ttl}</td>
              <td className="num py-1.5">{n.addr ?? "*"}</td>
              <td className="py-1.5 text-muted-foreground">{t(ZONE_TEXT[n.zone].name)}</td>
              <td className={cn("num py-1.5 text-right", n.loss > 0 && "text-warning")}>
                {n.loss}%
              </td>
              <td className="num py-1.5 text-right font-semibold">{ms(n.avg)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
