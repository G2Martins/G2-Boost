import { Activity, Gamepad2, Loader2, Play, Route, Square } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { Gauge } from "@/components/ui/gauge";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  MetricRow,
  StatusPill,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import { useI18n } from "@/lib/i18n";
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
  return (
    <PageHeader
      title={t("network.title")}
      subtitle={t("network.subtitle")}
      tabs={
        <Tabs defaultValue="scanner">
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
              <Card className="animate-rise">
                <EmptyState
                  icon={<Gamepad2 className="size-5" />}
                  title={t("common.soon")}
                  body={t("network.games.soon")}
                />
              </Card>
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

function Monitor() {
  const { t } = useI18n();
  return (
    <div className="grid grid-cols-1 gap-4 lg:grid-cols-[320px_1fr]">
      <Card className="animate-rise">
        <CardTitle help={t("network.health.help")}>{t("network.health")}</CardTitle>
        <StatusPill tone="neutral" className="w-full justify-center">
          {t("common.soon")}
        </StatusPill>
        <div className="mt-2 divide-y divide-border">
          <MetricRow label={t("network.latency")} value="—" />
          <MetricRow label={t("network.loss")} value="—" />
          <MetricRow label={t("network.jitter")} value="—" />
          <MetricRow label={t("network.spikes")} value="—" />
        </div>
      </Card>
      <Card className="animate-rise" style={rise(1)}>
        <CardTitle>{t("network.chart")}</CardTitle>
        <EmptyState
          icon={<Activity className="size-5" />}
          title={t("common.soon")}
          body={t("network.monitor.soon")}
        />
      </Card>
    </div>
  );
}

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

  const phaseKey = (p: Phase) => `network.scanner.phase.${p}` as const;
  const down = result?.downloadMbps ?? progress?.downloadMbps ?? 0;
  const up = result?.uploadMbps ?? progress?.uploadMbps ?? 0;
  const meta = result?.meta ?? progress?.meta;

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
              [
                t("network.avg_ping"),
                result
                  ? formatMs(result.latencyMs, locale)
                  : progress?.latencyMs
                    ? formatMs(progress.latencyMs, locale)
                    : "—",
              ],
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

function Tools() {
  const { t } = useI18n();
  const [tool, setTool] = useState<"MTR" | "Ping" | "TCPing">("MTR");
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
            defaultValue="www.google.com"
          />
        </label>
        <Button variant="primary" disabled title={t("common.soon")}>
          {t("common.start")}
          <Play className="size-4" />
        </Button>
      </Card>
      <Card className="animate-rise" style={rise(1)}>
        <EmptyState
          icon={<Route className="size-5" />}
          title={t("network.tools.empty.title", { tool })}
          body={t("network.tools.empty.body")}
          action={<Badge className="bg-card-hover text-muted-foreground">{t("common.soon")}</Badge>}
        />
      </Card>
    </>
  );
}
