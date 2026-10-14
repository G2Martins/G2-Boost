import { Activity, Gamepad2, Loader2, Play, Power, Route, Square } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Gauge } from "@/components/ui/gauge";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  HelpHint,
  MetricRow,
  StatusPill,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import { backend, type Hop, inTauri, type PingReport } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import {
  activityVerdicts,
  type Phase,
  type Progress,
  runSpeedTest,
  type SpeedResult,
} from "@/lib/speedtest";
import { cn, formatBytes, formatMbps, formatMs } from "@/lib/utils";

export function NetworkPage() {
  const { t } = useI18n();
  const initial = ["monitor", "scanner", "games", "tools"].includes(boot.startTab ?? "")
    ? (boot.startTab as string)
    : "monitor";
  return (
    <PageHeader
      title={t("network.title")}
      subtitle={t("network.subtitle")}
      tabs={
        <Tabs defaultValue={initial}>
          <TabsList>
            <TabsTrigger value="monitor">{t("network.tab.monitor")}</TabsTrigger>
            <TabsTrigger value="scanner">{t("network.tab.scanner")}</TabsTrigger>
            <TabsTrigger value="games">{t("network.tab.games")}</TabsTrigger>
            <TabsTrigger value="tools">{t("network.tab.tools")}</TabsTrigger>
          </TabsList>
          <TabsContent value="monitor">
            <PageBody className="px-0 pb-0">
              <Monitor />
            </PageBody>
          </TabsContent>
          <TabsContent value="scanner">
            <PageBody className="px-0 pb-0">
              <Scanner />
            </PageBody>
          </TabsContent>
          <TabsContent value="games">
            <PageBody className="px-0 pb-0">
              <Games />
            </PageBody>
          </TabsContent>
          <TabsContent value="tools">
            <PageBody className="px-0 pb-0">
              <Tools />
            </PageBody>
          </TabsContent>
        </Tabs>
      }
    />
  );
}

/* ================= Monitor (ICMP every second, 60-sample window) ================= */

const TARGETS = [
  { id: "google", host: "8.8.8.8", label: "Google", color: "hsl(var(--chart-1))" },
  { id: "cloudflare", host: "1.1.1.1", label: "Cloudflare", color: "hsl(var(--chart-2))" },
] as const;
const WINDOW = 60;
type Sample = { t: number; rtt: (number | null)[] };

function statsOf(values: (number | null)[]) {
  const ok = values.filter((v): v is number => v !== null);
  const n = values.length;
  if (!ok.length) {
    return { avg: null, min: null, max: null, jitter: null, loss: n ? 100 : 0, spikes: 0 };
  }
  const sorted = [...ok].sort((a, b) => a - b);
  const mid = sorted.length / 2;
  const median =
    sorted.length % 2
      ? (sorted[Math.floor(mid)] as number)
      : ((sorted[mid - 1] as number) + (sorted[mid] as number)) / 2;
  let jit = 0;
  for (let i = 1; i < ok.length; i++) jit += Math.abs((ok[i] as number) - (ok[i - 1] as number));
  return {
    avg: ok.reduce((a, b) => a + b, 0) / ok.length,
    min: sorted[0] as number,
    max: sorted[sorted.length - 1] as number,
    jitter: ok.length > 1 ? jit / (ok.length - 1) : 0,
    loss: (100 * (n - ok.length)) / n,
    spikes: ok.filter((v) => v > median * 2 && v > median + 10).length,
  };
}

function Monitor() {
  const { t, locale } = useI18n();
  const [on, setOn] = useState(inTauri);
  const [samples, setSamples] = useState<Sample[]>([]);
  const timer = useRef<number | null>(null);

  useEffect(() => {
    if (!on) return;
    let alive = true;
    const tick = async () => {
      const rtt = await Promise.all(
        TARGETS.map((tg) =>
          backend
            .pingHost(tg.host, 1, 900, 0)
            .then((r) => (r.probes[0]?.kind === "reply" ? r.probes[0].rtt_ms : null))
            .catch(() => null),
        ),
      );
      if (!alive) return;
      setSamples((s) => [...s.slice(-(WINDOW - 1)), { t: Date.now(), rtt }]);
      timer.current = window.setTimeout(tick, 1000);
    };
    tick();
    return () => {
      alive = false;
      if (timer.current) window.clearTimeout(timer.current);
    };
  }, [on]);

  const primary = useMemo(() => statsOf(samples.map((s) => s.rtt[0] ?? null)), [samples]);
  const enough = samples.length >= 10;
  const stable = enough && primary.loss <= 5 && (primary.jitter ?? 0) <= 15 && primary.spikes === 0;
  const fmt = (v: number | null) => (v === null ? "—" : formatMs(v, locale));

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-4 py-4">
        <span className="font-display text-[15px] font-semibold tracking-wide">
          {t("network.monitor.title")}
        </span>
        <Button variant="outline" size="sm" onClick={() => setOn((v) => !v)} disabled={!inTauri}>
          <Power className="size-4" />
          {on ? t("network.monitor.off") : t("network.monitor.on")}
        </Button>
        {!inTauri ? (
          <span className="text-xs text-muted-foreground">{t("common.app_only")}</span>
        ) : null}
        <div className="ml-auto flex gap-2">
          <Badge className="bg-card-hover text-foreground">
            {t("network.monitor.targets.internet")}
          </Badge>
          <Badge className="bg-card-hover text-muted-foreground" title={t("common.soon")}>
            {t("network.monitor.targets.gateway")}
          </Badge>
          <Badge className="bg-card-hover text-muted-foreground" title={t("common.soon")}>
            {t("network.monitor.targets.aws")}
          </Badge>
        </div>
      </Card>
      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[320px_1fr]">
        <Card className="animate-rise" style={rise(1)}>
          <CardTitle help={t("network.health.help")}>{t("network.health")}</CardTitle>
          <StatusPill
            tone={!enough ? "neutral" : stable ? "success" : "warning"}
            className="w-full justify-center"
          >
            {!enough
              ? t("network.monitor.waiting")
              : stable
                ? t("network.stable")
                : t("network.unstable")}
          </StatusPill>
          <div className="mt-2 divide-y divide-border">
            <MetricRow
              label={t("network.latency")}
              value={fmt(primary.avg)}
              ok={enough ? (primary.avg ?? 999) < 100 : undefined}
            />
            <MetricRow
              label={t("network.loss")}
              value={enough ? `${primary.loss.toFixed(0)} %` : "—"}
              ok={enough ? primary.loss <= 5 : undefined}
            />
            <MetricRow
              label={t("network.jitter")}
              value={fmt(primary.jitter)}
              ok={enough ? (primary.jitter ?? 99) <= 15 : undefined}
            />
            <MetricRow
              label={t("network.spikes")}
              value={enough ? String(primary.spikes) : "—"}
              ok={enough ? primary.spikes === 0 : undefined}
            />
          </div>
        </Card>
        <Card className="animate-rise" style={rise(2)}>
          <CardTitle
            right={
              <span className="flex gap-4 text-xs">
                {TARGETS.map((tg) => (
                  <span key={tg.id} className="flex items-center gap-1.5">
                    <span className="size-2 rounded-full" style={{ background: tg.color }} />
                    {tg.label}
                  </span>
                ))}
              </span>
            }
          >
            {t("network.chart")}
          </CardTitle>
          {samples.length ? (
            <LatencyChart samples={samples} />
          ) : (
            <EmptyState
              icon={<Activity className="size-5" />}
              title={on ? t("network.monitor.waiting") : t("network.monitor.title")}
            />
          )}
          <div className="mt-2 flex flex-wrap gap-6 text-xs text-muted-foreground">
            <span>
              {t("network.avg")} <b className="num text-foreground">{fmt(primary.avg)}</b>
            </span>
            <span>
              {t("network.min")} <b className="num text-foreground">{fmt(primary.min)}</b>
            </span>
            <span>
              {t("network.max")} <b className="num text-foreground">{fmt(primary.max)}</b>
            </span>
            <span className="ml-auto flex items-center gap-4">
              <span className="flex items-center gap-1.5">
                <span className="h-2 w-3 bg-chart-3" />
                {t("network.monitor.legend.availability")}
              </span>
              <span className="flex items-center gap-1.5">
                <span className="h-2 w-3 bg-chart-2" />
                {t("network.monitor.legend.loss")}
              </span>
            </span>
          </div>
        </Card>
      </div>
    </>
  );
}

function LatencyChart({ samples }: { samples: Sample[] }) {
  const W = 800;
  const H = 220;
  const padL = 36;
  const padB = 28;
  const maxRtt = Math.max(
    20,
    ...samples.flatMap((s) => s.rtt.filter((v): v is number => v !== null)),
  );
  const yMax = Math.ceil((maxRtt * 1.2) / 10) * 10;
  const x = (i: number) => padL + (i / (WINDOW - 1)) * (W - padL - 8);
  const y = (v: number) => 8 + (1 - v / yMax) * (H - padB - 8);
  const line = (idx: number) => {
    let d = "";
    let pen = false;
    samples.forEach((s, i) => {
      const v = s.rtt[idx];
      if (v === null || v === undefined) {
        pen = false;
        return;
      }
      d += `${pen ? "L" : "M"}${x(i).toFixed(1)},${y(v).toFixed(1)} `;
      pen = true;
    });
    return d;
  };
  const ticks = [0, yMax / 2, yMax];
  return (
    <svg viewBox={`0 0 ${W} ${H}`} className="h-56 w-full" role="img" aria-label="latency">
      {ticks.map((v) => (
        <g key={v}>
          <line
            x1={padL}
            x2={W - 8}
            y1={y(v)}
            y2={y(v)}
            stroke="hsl(var(--border))"
            strokeDasharray="2 4"
          />
          <text
            x={padL - 6}
            y={y(v) + 4}
            textAnchor="end"
            fontSize="10"
            fill="hsl(var(--muted-foreground))"
            className="num"
          >
            {v}
          </text>
        </g>
      ))}
      {TARGETS.map((tg, idx) => (
        <path
          key={tg.id}
          d={line(idx)}
          fill="none"
          stroke={tg.color}
          strokeWidth={1.8}
          strokeLinejoin="round"
        />
      ))}
      {samples.map((s, i) => {
        const lost = s.rtt.some((v) => v === null);
        return (
          <rect
            key={s.t}
            x={x(i) - 3}
            y={H - padB + 8}
            width={6}
            height={6}
            fill={lost ? "hsl(var(--chart-2))" : "hsl(var(--chart-3))"}
          />
        );
      })}
    </svg>
  );
}

/* ================= Scanner (Cloudflare speed test) ================= */

function Scanner() {
  const { t, locale } = useI18n();
  const [progress, setProgress] = useState<Progress | null>(null);
  const [result, setResult] = useState<SpeedResult | null>(null);
  const [error, setError] = useState<string | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const running = progress !== null && progress.phase !== "done";

  const start = useCallback(async () => {
    const ctrl = new AbortController();
    abortRef.current = ctrl;
    setError(null);
    setResult(null);
    try {
      const r = await runSpeedTest(setProgress, ctrl.signal);
      setResult(r);
    } catch (e) {
      if (!ctrl.signal.aborted) setError(e instanceof Error ? e.message : String(e));
      setProgress(null);
    }
  }, []);

  const stop = () => {
    abortRef.current?.abort();
    setProgress(null);
  };

  const phaseKey = (p: Phase) => `network.scanner.phase.${p}` as I18nKey;
  const down = result?.downloadMbps ?? progress?.downloadMbps ?? 0;
  const up = result?.uploadMbps ?? progress?.uploadMbps ?? 0;
  const meta = result?.meta ?? progress?.meta;
  const idle = result
    ? formatMs(result.latencyMs, locale)
    : progress?.latencyMs
      ? formatMs(progress.latencyMs, locale)
      : "—";

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-4 py-4">
        <span className="font-display text-[15px] font-semibold tracking-wide">
          {t("network.scanner.title")}
        </span>
        <span className="text-xs text-muted-foreground">
          {running ? (
            <span className="inline-flex items-center gap-2">
              <Loader2 className="size-3.5 animate-spin text-primary" />
              {t(phaseKey(progress.phase))}
            </span>
          ) : (
            t("network.scanner.help")
          )}
        </span>
        <div className="ml-auto">
          {running ? (
            <Button variant="outline" onClick={stop}>
              <Square className="size-4" />
              {t("common.stop")}
            </Button>
          ) : (
            <Button variant="primary" size="lg" onClick={start}>
              <Play className="size-4" />
              {t("common.start")}
            </Button>
          )}
        </div>
      </Card>

      {error ? (
        <Card className="border-destructive/40 text-destructive">
          {t("network.scanner.error", { error })}
        </Card>
      ) : null}

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
        <Card className="animate-rise" style={rise(1)}>
          <CardTitle>{t("network.download")}</CardTitle>
          <div className="flex justify-center">
            <Gauge
              value={down}
              max={Math.max(100, niceMax(down))}
              label={t("network.download")}
              valueText={formatMbps(down, locale)}
              tone="download"
            />
          </div>
          <Stats
            rows={[
              [t("network.data_used"), result ? formatBytes(result.bytesDown, locale) : "—"],
              [t("network.avg_ping"), idle],
              [t("network.variation"), result ? formatMs(result.jitterMs, locale) : "—"],
            ]}
          />
        </Card>
        <Card className="animate-rise" style={rise(2)}>
          <CardTitle>{t("network.upload")}</CardTitle>
          <div className="flex justify-center">
            <Gauge
              value={up}
              max={Math.max(100, niceMax(up))}
              label={t("network.upload")}
              valueText={formatMbps(up, locale)}
              tone="upload"
            />
          </div>
          <Stats
            rows={[
              [t("network.data_used"), result ? formatBytes(result.bytesUp, locale) : "—"],
              [t("network.avg_ping"), result ? formatMs(result.loadedUpMs, locale) : "—"],
              [
                t("network.variation"),
                result ? formatMs(Math.abs(result.loadedUpMs - result.latencyMs), locale) : "—",
              ],
            ]}
          />
        </Card>
      </div>

      <Card className="animate-rise flex flex-wrap gap-x-8 gap-y-2 py-4 text-xs" style={rise(3)}>
        <Meta label={t("network.isp")} value={meta?.isp ?? (meta?.asn ? `AS${meta.asn}` : "—")} />
        <Meta label={t("network.ip")} value={meta?.ip ?? "—"} />
        <Meta
          label={t("network.location")}
          value={meta ? [meta.city, meta.country].filter(Boolean).join(", ") || "—" : "—"}
        />
      </Card>

      <div className="grid grid-cols-1 gap-4 lg:grid-cols-[1fr_280px_280px]">
        <Card className="animate-rise" style={rise(4)}>
          <CardTitle>{t("network.activity")}</CardTitle>
          <ActivityTable result={result} />
        </Card>
        <Card className="animate-rise" style={rise(5)}>
          <CardTitle>{t("network.latency")}</CardTitle>
          <div className="flex flex-col gap-4 text-xs">
            <Lat
              label={t("network.latency.unloaded")}
              value={result ? formatMs(result.latencyMs, locale) : "—"}
            />
            <Lat
              label={t("network.latency.loaded_down")}
              value={result ? formatMs(result.loadedDownMs, locale) : "—"}
            />
            <Lat
              label={t("network.latency.loaded_up")}
              value={result ? formatMs(result.loadedUpMs, locale) : "—"}
            />
          </div>
        </Card>
        <Card className="animate-rise" style={rise(6)}>
          <CardTitle help={t("network.bufferbloat.help")}>{t("network.bufferbloat")}</CardTitle>
          <div className="flex justify-center py-2">
            <div
              className={cn(
                "flex size-36 items-center justify-center rounded-full border-4 font-display text-5xl font-bold",
                result ? gradeClass(result.grade) : "border-border text-muted-foreground",
              )}
            >
              {result?.grade ?? "—"}
            </div>
          </div>
        </Card>
      </div>
    </>
  );
}

function niceMax(v: number) {
  if (v <= 0) return 100;
  const p = 10 ** Math.floor(Math.log10(v));
  return Math.ceil(v / p) * p;
}

function gradeClass(g: SpeedResult["grade"]) {
  if (g === "A+" || g === "A") return "border-success text-success";
  if (g === "B" || g === "C") return "border-warning text-warning";
  return "border-destructive text-destructive";
}

function Stats({ rows }: { rows: [string, string][] }) {
  return (
    <div className="mt-2 flex justify-between gap-4 text-xs">
      {rows.map(([k, v]) => (
        <span key={k} className="text-muted-foreground">
          {k}: <span className="num font-semibold text-foreground">{v}</span>
        </span>
      ))}
    </div>
  );
}

function Meta({ label, value }: { label: string; value: string }) {
  return (
    <span className="text-muted-foreground">
      {label}: <span className="num font-semibold text-foreground">{value}</span>
    </span>
  );
}

function Lat({ label, value }: { label: string; value: string }) {
  return (
    <div className="flex items-center gap-3">
      <Route className="size-4 text-muted-foreground" />
      <div>
        <div className="text-muted-foreground">{label}</div>
        <div className="num font-semibold">{value}</div>
      </div>
    </div>
  );
}

function ActivityTable({ result }: { result: SpeedResult | null }) {
  const { t } = useI18n();
  const v = result ? activityVerdicts(result) : null;
  const rows = [
    ["web", t("network.activity.web")],
    ["audio", t("network.activity.audio")],
    ["video4k", t("network.activity.video4k")],
    ["conf", t("network.activity.conf")],
    ["gaming", t("network.activity.gaming")],
  ] as const;
  const mark = (ok?: boolean) =>
    ok === undefined ? (
      <span className="text-muted-foreground">—</span>
    ) : ok ? (
      <span className="text-success">✓</span>
    ) : (
      <span className="text-destructive">✕</span>
    );
  return (
    <table className="w-full text-xs">
      <thead className="text-muted-foreground">
        <tr className="border-b border-border">
          <th className="py-2 text-left font-medium">{t("network.activity")}</th>
          <th className="py-2 text-left font-medium">{t("network.activity.ideal")}</th>
          <th className="py-2 text-left font-medium">{t("network.activity.loaded")}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map(([k, label]) => (
          <tr key={k} className="border-b border-border/60 last:border-0">
            <td className="py-2.5">{label}</td>
            <td className="py-2.5 font-bold">{mark(v?.[k].ideal)}</td>
            <td className="py-2.5 font-bold">{mark(v?.[k].loaded)}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/* ================= Games (Steam Datagram Relay POPs, pinged natively) ================= */

const GAMES = [
  { appid: 730, name: "Counter-Strike 2", publisher: "Valve" },
  { appid: 570, name: "Dota 2", publisher: "Valve" },
  { appid: 1422450, name: "Deadlock", publisher: "Valve" },
] as const;

type SdrConfig = {
  pops?: Record<string, { desc?: string; geo?: [number, number]; relays?: { ipv4: string }[] }>;
};
type ServerRow = {
  code: string;
  desc: string;
  region: I18nKey;
  ip: string;
  rtt: number | null;
  loss: number;
};

function regionOf(geo?: [number, number]): I18nKey {
  if (!geo) return "network.games.region.other";
  const [lon, lat] = geo;
  if (lon < -30 && lat < 13) return "network.games.region.sa";
  if (lon < -30) return "network.games.region.na";
  if (lon < 45 && lat > 34) return "network.games.region.eu";
  if (lon >= 45 || lat > 34) return "network.games.region.asia";
  return "network.games.region.other";
}

const REGION_ORDER: I18nKey[] = [
  "network.games.region.sa",
  "network.games.region.na",
  "network.games.region.eu",
  "network.games.region.asia",
  "network.games.region.other",
];

function Games() {
  const { t, locale } = useI18n();
  const [appid, setAppid] = useState<number | null>(null);
  const [rows, setRows] = useState<ServerRow[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [when, setWhen] = useState<Date | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: dev autorun fires once on mount
  useEffect(() => {
    if (boot.autorun) void run(730);
  }, []);

  const run = async (id: number) => {
    setAppid(id);
    setBusy(true);
    setError(null);
    setRows([]);
    setWhen(null);
    try {
      const cfg = await backend.fetchJson<SdrConfig>(
        `https://api.steampowered.com/ISteamApps/GetSDRConfig/v1/?appid=${id}`,
      );
      const pops = Object.entries(cfg.pops ?? {}).flatMap(([code, p]) => {
        const ip = p.relays?.[0]?.ipv4;
        return typeof ip === "string"
          ? [{ code, desc: p.desc ?? code, region: regionOf(p.geo), ip }]
          : [];
      });
      const out: ServerRow[] = [];
      for (let i = 0; i < pops.length; i += 8) {
        const chunk = pops.slice(i, i + 8);
        const res = await Promise.all(
          chunk.map(async (p) => {
            try {
              const r = await backend.pingHost(p.ip, 3, 1200, 80);
              return { ...p, rtt: r.stats.avg_ms, loss: r.stats.loss_pct };
            } catch {
              return { ...p, rtt: null, loss: 100 };
            }
          }),
        );
        out.push(...res);
        setRows([...out]);
      }
      setWhen(new Date());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const grouped = useMemo(() => {
    const m = new Map<I18nKey, ServerRow[]>();
    for (const r of rows) m.set(r.region, [...(m.get(r.region) ?? []), r]);
    for (const list of m.values()) list.sort((a, b) => (a.rtt ?? 9999) - (b.rtt ?? 9999));
    return REGION_ORDER.filter((k) => m.has(k)).map((k) => [k, m.get(k) as ServerRow[]] as const);
  }, [rows]);

  const game = GAMES.find((g) => g.appid === appid);
  const dot = (rtt: number | null) =>
    rtt === null
      ? "bg-destructive"
      : rtt < 60
        ? "bg-success"
        : rtt < 120
          ? "bg-warning"
          : "bg-destructive";

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="font-display text-[15px] font-semibold tracking-wide">
          {t("network.games.pick")}
        </span>
        <span className="text-xs text-muted-foreground">{t("network.games.source")}</span>
        <div className="ml-auto flex gap-2">
          {GAMES.map((g) => (
            <Button
              key={g.appid}
              variant={appid === g.appid ? "primary" : "outline"}
              onClick={() => run(g.appid)}
              disabled={busy || !inTauri}
            >
              <Gamepad2 className="size-4" />
              {g.name}
            </Button>
          ))}
        </div>
        {!inTauri ? (
          <span className="w-full text-xs text-muted-foreground">{t("common.app_only")}</span>
        ) : null}
      </Card>
      {error ? (
        <Card className="border-destructive/40 text-destructive">
          {t("network.games.error", { error })}
        </Card>
      ) : null}
      <Card className="animate-rise" style={rise(1)}>
        <CardTitle
          right={
            when ? (
              <span className="text-xs text-muted-foreground">
                {t("common.updated_at", { when: when.toLocaleString(locale) })}
              </span>
            ) : busy ? (
              <span className="inline-flex items-center gap-2 text-xs text-muted-foreground">
                <Loader2 className="size-3.5 animate-spin text-primary" />
                {t("network.games.loading")}
              </span>
            ) : null
          }
        >
          {t("network.games.last")}
          {game ? (
            <span className="ml-2 font-sans text-xs font-normal text-muted-foreground">
              {game.name}
            </span>
          ) : null}
        </CardTitle>
        {rows.length === 0 && !busy ? (
          <EmptyState
            icon={<Gamepad2 className="size-5" />}
            title={t("network.games.pick")}
            body={t("network.games.hint")}
          />
        ) : (
          <div className="flex flex-col gap-3">
            <div className="flex justify-between px-3 text-xs text-muted-foreground">
              <span>{t("network.games.servers")}</span>
              <span>Ethernet</span>
            </div>
            {grouped.map(([region, list]) => (
              <div key={region} className="rounded-md border border-border">
                <div className="flex items-center gap-2 bg-card-hover px-3 py-2 font-display text-[13px] font-semibold">
                  {t(region)}
                  <Badge className="bg-background text-muted-foreground">{list.length}</Badge>
                </div>
                {list.map((r) => (
                  <div
                    key={r.code}
                    className="flex items-center gap-3 border-t border-border/60 px-3 py-2 text-xs"
                  >
                    <span className="flex flex-col">
                      <span className="font-semibold">{r.desc}</span>
                      <span className="text-muted-foreground">
                        {game?.publisher} · {r.code} · <span className="num">{r.ip}</span>
                      </span>
                    </span>
                    <span className="ml-auto flex items-center gap-2">
                      <span className={cn("size-2 rounded-full", dot(r.rtt))} />
                      <span className="num font-semibold">
                        {r.rtt === null ? t("network.tools.timeout") : formatMs(r.rtt, locale)}
                      </span>
                      {r.loss > 0 && r.rtt !== null ? (
                        <span className="text-muted-foreground">({r.loss.toFixed(0)} %)</span>
                      ) : null}
                    </span>
                  </div>
                ))}
              </div>
            ))}
          </div>
        )}
      </Card>
    </>
  );
}

/* ================= Tools (MTR / Ping / TCPing) ================= */

type Tool = "MTR" | "Ping" | "TCPing";

function hopStats(h: Hop) {
  const ok = h.rtts_ms.filter((v): v is number => v !== null);
  return {
    loss: (100 * (h.rtts_ms.length - ok.length)) / h.rtts_ms.length,
    avg: ok.length ? ok.reduce((a, b) => a + b, 0) / ok.length : null,
    min: ok.length ? Math.min(...ok) : null,
    max: ok.length ? Math.max(...ok) : null,
  };
}

function Tools() {
  const { t, locale } = useI18n();
  const [tool, setTool] = useState<Tool>("MTR");
  const [target, setTarget] = useState("www.google.com");
  const [busy, setBusy] = useState(false);
  const [ping, setPing] = useState<PingReport | null>(null);
  const [hops, setHops] = useState<Hop[] | null>(null);
  // biome-ignore lint/correctness/useExhaustiveDependencies: dev autorun fires once on mount
  useEffect(() => {
    if (boot.autorun) void start();
  }, []);
  const [error, setError] = useState<string | null>(null);
  const fmt = (v: number | null) => (v === null ? "—" : formatMs(v, locale));
  const th = (label: string, right = false) => (
    <th className={cn("py-2 font-medium", right ? "text-right" : "text-left")}>{label}</th>
  );

  const start = async () => {
    setBusy(true);
    setError(null);
    setPing(null);
    setHops(null);
    try {
      if (tool === "Ping") setPing(await backend.pingHost(target.trim(), 10, 1500, 300));
      else if (tool === "MTR") setHops(await backend.traceHost(target.trim(), 30, 3, 1500));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="font-display text-[15px] font-semibold tracking-wide">
          {t("network.tools.pick")}
        </span>
        <div className="flex gap-2">
          {(["MTR", "Ping", "TCPing"] as const).map((x) => (
            <button
              key={x}
              type="button"
              onClick={() => setTool(x)}
              className={cn(
                "rounded-md border px-3 py-2 font-display text-[13px] font-semibold",
                tool === x
                  ? "border-ring bg-card-hover"
                  : "border-border bg-background text-muted-foreground hover:text-foreground",
              )}
            >
              {x}
            </button>
          ))}
        </div>
        <label className="ml-auto flex h-10 w-64 flex-col justify-center rounded-md border border-border bg-background px-3">
          <span className="text-[10px] text-muted-foreground">{t("network.tools.target")}</span>
          <input
            className="bg-transparent text-[13px] outline-none"
            value={target}
            onChange={(e) => setTarget(e.target.value)}
            maxLength={253}
            spellCheck={false}
          />
        </label>
        <Button
          variant="primary"
          onClick={start}
          disabled={busy || !inTauri || tool === "TCPing" || !target.trim()}
          title={tool === "TCPing" ? t("common.soon") : undefined}
        >
          {busy ? <Loader2 className="size-4 animate-spin" /> : <Play className="size-4" />}
          {t("common.start")}
        </Button>
      </Card>
      {error ? (
        <Card className="border-destructive/40 text-destructive">
          {t("network.tools.error", { error })}
        </Card>
      ) : null}
      <Card className="animate-rise" style={rise(1)}>
        {ping ? (
          <>
            <CardTitle right={<HelpHint text={t("network.health.help")} />}>
              Ping ·{" "}
              <span className="num font-sans text-xs font-normal text-muted-foreground">
                {ping.host} ({ping.ip})
              </span>
            </CardTitle>
            <table className="w-full text-xs">
              <thead className="text-muted-foreground">
                <tr className="border-b border-border">
                  {th("#")}
                  {th(t("network.tools.from"))}
                  {th(t("network.tools.rtt"), true)}
                </tr>
              </thead>
              <tbody>
                {ping.probes
                  .map((p, i) => ({ seq: i + 1, p }))
                  .map(({ seq, p }) => (
                    <tr key={seq} className="border-b border-border/60 last:border-0">
                      <td className="num py-1.5">{seq}</td>
                      <td className="num py-1.5">{"from" in p ? p.from : "—"}</td>
                      <td
                        className={cn(
                          "num py-1.5 text-right font-semibold",
                          p.kind !== "reply" && "text-destructive",
                        )}
                      >
                        {p.kind === "reply"
                          ? formatMs(p.rtt_ms, locale)
                          : t("network.tools.timeout")}
                      </td>
                    </tr>
                  ))}
              </tbody>
            </table>
            <div className="mt-3 flex flex-wrap gap-6 text-xs text-muted-foreground">
              <span>
                {t("network.avg")} <b className="num text-foreground">{fmt(ping.stats.avg_ms)}</b>
              </span>
              <span>
                {t("network.min")} <b className="num text-foreground">{fmt(ping.stats.min_ms)}</b>
              </span>
              <span>
                {t("network.max")} <b className="num text-foreground">{fmt(ping.stats.max_ms)}</b>
              </span>
              <span>
                {t("network.jitter")}{" "}
                <b className="num text-foreground">{fmt(ping.stats.jitter_ms)}</b>
              </span>
              <span>
                {t("network.loss")}{" "}
                <b className="num text-foreground">{ping.stats.loss_pct.toFixed(0)} %</b>
              </span>
            </div>
          </>
        ) : hops ? (
          <>
            <CardTitle>
              MTR ·{" "}
              <span className="num font-sans text-xs font-normal text-muted-foreground">
                {target}
              </span>
            </CardTitle>
            <table className="w-full text-xs">
              <thead className="text-muted-foreground">
                <tr className="border-b border-border">
                  {th(t("network.tools.hop"))}
                  {th(t("network.tools.from"))}
                  {th(t("network.tools.loss"), true)}
                  {th(t("network.avg"), true)}
                  {th(t("network.min"), true)}
                  {th(t("network.max"), true)}
                </tr>
              </thead>
              <tbody>
                {hops.map((h) => {
                  const s = hopStats(h);
                  return (
                    <tr key={h.ttl} className="border-b border-border/60 last:border-0">
                      <td className="num py-1.5">{h.ttl}</td>
                      <td className="num py-1.5">{h.addr ?? "*"}</td>
                      <td className={cn("num py-1.5 text-right", s.loss > 0 && "text-warning")}>
                        {s.loss.toFixed(0)} %
                      </td>
                      <td className="num py-1.5 text-right font-semibold">{fmt(s.avg)}</td>
                      <td className="num py-1.5 text-right">{fmt(s.min)}</td>
                      <td className="num py-1.5 text-right">{fmt(s.max)}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </>
        ) : (
          <EmptyState
            icon={
              busy ? (
                <Loader2 className="size-5 animate-spin text-primary" />
              ) : (
                <Route className="size-5" />
              )
            }
            title={
              busy ? t("network.tools.running", { tool }) : t("network.tools.empty.title", { tool })
            }
            body={
              tool === "TCPing" ? t("network.tools.tcping_soon") : t("network.tools.empty.body")
            }
          />
        )}
      </Card>
    </>
  );
}
