import { Activity, RefreshCw, ShieldAlert, Wifi, WifiOff } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Card, CardTitle, EmptyState, Skeleton } from "@/components/ui/primitives";
import { backend, inTauri, type NetUsage, type WifiView } from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/* ================= Who is using the network ================= */

export function NetUsageTab() {
  const { t } = useI18n();
  const [data, setData] = useState<NetUsage | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(async () => {
    setBusy(true);
    setData((await backend.netUsage()) ?? { processes: [], tcp: 0, udp: 0 });
    setBusy(false);
  }, []);
  useEffect(() => {
    void load();
    const id = setInterval(() => void load(), 10_000);
    return () => clearInterval(id);
  }, [load]);
  const rows = (data?.processes ?? []).filter(
    (p) => p.established + p.listening_ports.length + p.udp_endpoints > 0,
  );
  return (
    <Card className="animate-rise">
      <CardTitle
        help={t("net.usage.help")}
        right={
          <Button size="sm" variant="ghost" disabled={busy || !inTauri} onClick={load}>
            <RefreshCw className={cn("size-4", busy && "animate-spin")} />
          </Button>
        }
      >
        <Activity className="size-4" />
        {t("net.usage.title")}
        {data ? (
          <span className="num text-[11px] font-normal text-muted-foreground">
            {t("net.usage.totals", { tcp: data.tcp, udp: data.udp })}
          </span>
        ) : null}
      </CardTitle>
      {data === null ? (
        [0, 1, 2].map((k) => <Skeleton key={k} className="my-2 h-9 w-full" />)
      ) : rows.length === 0 ? (
        <EmptyState icon={<Activity className="size-5" />} title={t("net.usage.empty")} body="" />
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-xs">
            <thead className="text-muted-foreground">
              <tr className="border-b border-border">
                <th className="py-2 text-left font-medium">{t("net.usage.col.app")}</th>
                <th className="py-2 text-right font-medium">{t("net.usage.col.active")}</th>
                <th className="py-2 text-right font-medium">{t("net.usage.col.internet")}</th>
                <th className="py-2 text-right font-medium">{t("net.usage.col.listening")}</th>
                <th className="py-2 text-right font-medium">{t("net.usage.col.udp")}</th>
              </tr>
            </thead>
            <tbody>
              {rows.slice(0, 60).map((p) => (
                <tr
                  key={p.exe_path || p.process}
                  className="border-b border-border/60 last:border-0"
                >
                  <td className="max-w-80 py-1.5">
                    <span className="block truncate font-semibold" title={p.exe_path || p.process}>
                      {p.process}
                    </span>
                  </td>
                  <td className="num py-1.5 text-right">{p.established || "—"}</td>
                  <td className="num py-1.5 text-right">{p.public_remotes || "—"}</td>
                  <td className="num py-1.5 text-right">
                    {p.listening_ports.length ? (
                      <span title={p.listening_ports.join(", ")}>
                        {p.listening_ports.slice(0, 4).join(", ")}
                        {p.listening_ports.length > 4 ? ` +${p.listening_ports.length - 4}` : ""}
                      </span>
                    ) : (
                      "—"
                    )}
                  </td>
                  <td className="num py-1.5 text-right">{p.udp_endpoints || "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <p className="mt-3 text-[11px] text-muted-foreground">{t("net.usage.note")}</p>
    </Card>
  );
}

/* ================= Wi-Fi: security, signal, channels ================= */

const BAND_LABEL: Record<string, string> = { ghz_2_4: "2,4 GHz", ghz_5: "5 GHz", ghz_6: "6 GHz" };

function SignalBars({ quality }: { quality: number }) {
  return (
    <span className="flex items-end gap-0.5" aria-hidden>
      {[25, 50, 75, 100].map((step, i) => (
        <span
          key={step}
          className={cn("w-1.5 rounded-sm", quality >= step - 12 ? "bg-primary" : "bg-border")}
          style={{ height: 6 + i * 4 }}
        />
      ))}
    </span>
  );
}

export function WifiTab() {
  const { t } = useI18n();
  const [data, setData] = useState<WifiView | null>(null);
  const [busy, setBusy] = useState(false);
  const load = useCallback(async (refresh: boolean) => {
    setBusy(true);
    setData(await backend.wifiReport(refresh));
    setBusy(false);
  }, []);
  useEffect(() => {
    void load(false);
  }, [load]);

  if (data?.error === "no_service" || (data && !data.status && !data.error)) {
    return (
      <Card className="animate-rise">
        <EmptyState
          icon={<WifiOff className="size-5" />}
          title={data.error ? t("wifi.none.title") : t("wifi.disconnected")}
          body={data.error ? t("wifi.none.body") : ""}
        />
      </Card>
    );
  }
  if (data?.error === "location_denied") {
    return (
      <Card className="animate-rise">
        <EmptyState
          icon={<ShieldAlert className="size-5" />}
          title={t("wifi.location.title")}
          body={t("wifi.location.body")}
        />
      </Card>
    );
  }
  const s = data?.status;
  const maxLoad = Math.max(1, ...(data?.channels ?? []).map((c) => c.load));
  return (
    <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
      <Card className="animate-rise">
        <CardTitle
          help={t("wifi.help")}
          right={
            <Button
              size="sm"
              variant="ghost"
              disabled={busy || !inTauri}
              onClick={() => load(true)}
            >
              <RefreshCw className={cn("size-4", busy && "animate-spin")} />
              {t("wifi.rescan")}
            </Button>
          }
        >
          <Wifi className="size-4" />
          {t("wifi.title")}
        </CardTitle>
        {!s ? (
          <Skeleton className="h-24 w-full" />
        ) : (
          <>
            <div className="flex items-center gap-4">
              <SignalBars quality={s.signal_quality} />
              <div className="min-w-0 flex-1">
                <p className="truncate font-display text-lg font-semibold">{s.ssid || "—"}</p>
                <p className="num text-xs text-muted-foreground">
                  {s.signal_quality}%{s.rssi_dbm !== null ? ` · ${s.rssi_dbm} dBm` : ""}
                  {s.band ? ` · ${BAND_LABEL[s.band]}` : ""}
                  {s.channel ? ` · ${t("wifi.channel", { n: s.channel })}` : ""}
                  {` · ${s.phy.toUpperCase()}`}
                  {s.rx_mbps ? ` · ${s.rx_mbps} Mbps` : ""}
                </p>
              </div>
              <Badge tone={["open", "wep"].includes(s.auth) ? "warning" : "neutral"}>
                {s.auth.replaceAll("_", " ").toUpperCase()}
              </Badge>
            </div>
            <ul className="mt-4 space-y-2">
              {data?.issues.length === 0 ? (
                <li className="text-sm text-success">{t("wifi.ok")}</li>
              ) : null}
              {data?.issues.map((i) => (
                <li
                  key={i.kind}
                  className={cn(
                    "rounded-md px-3 py-2 text-xs ring-1 ring-inset",
                    i.severity === "critical"
                      ? "bg-destructive/10 text-destructive ring-destructive/25"
                      : i.severity === "warning"
                        ? "bg-warning/10 text-warning ring-warning/25"
                        : "bg-foreground/4 text-foreground/90 ring-border",
                  )}
                >
                  {t(`wifi.issue.${i.kind}` as I18nKey)}
                </li>
              ))}
            </ul>
          </>
        )}
      </Card>

      <Card className="animate-rise">
        <CardTitle help={t("wifi.channels.help")}>{t("wifi.channels.title")}</CardTitle>
        {data && data.channels.length > 0 ? (
          <>
            <div className="flex h-36 items-end gap-1">
              {data.channels.map((c) => (
                <div key={c.channel} className="flex flex-1 flex-col items-center gap-1">
                  <span
                    className={cn(
                      "w-full rounded-t-sm",
                      c.channel === s?.channel
                        ? "bg-primary"
                        : c.channel === data.best_channel
                          ? "bg-success/70"
                          : "bg-foreground/20",
                    )}
                    style={{ height: `${Math.max(4, (c.load / maxLoad) * 112)}px` }}
                    title={t("wifi.channel.networks", { n: c.networks })}
                  />
                  <span className="num text-[9px] text-muted-foreground">{c.channel}</span>
                </div>
              ))}
            </div>
            <p className="mt-3 text-xs">
              {data.best_channel && s?.channel && data.best_channel !== s.channel
                ? t("wifi.best", { best: data.best_channel, now: s.channel })
                : t("wifi.best.same")}
            </p>
            <p className="mt-1 text-[11px] text-muted-foreground">
              {t("wifi.networks.count", { n: data.networks.length })}
            </p>
          </>
        ) : (
          <p className="text-xs text-muted-foreground">{t("wifi.channels.empty")}</p>
        )}
      </Card>
    </div>
  );
}
