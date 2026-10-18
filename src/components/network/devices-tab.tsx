import {
  Camera,
  CircleHelp,
  Gamepad2,
  HardDrive,
  Laptop,
  Lightbulb,
  Loader2,
  type LucideIcon,
  Monitor,
  Printer,
  Radar,
  Router,
  ShieldAlert,
  Smartphone,
  Speaker,
  Tv,
} from "lucide-react";
import { useMemo, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { LuliCard } from "@/components/luli-card";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, EmptyState } from "@/components/ui/primitives";
import {
  backend,
  type DeviceKind,
  type DeviceScan,
  inTauri,
  type NetDevice,
  type OpenPort,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { usePageData } from "@/lib/luli";
import { cn } from "@/lib/utils";

const ICONS: Record<DeviceKind, LucideIcon> = {
  router: Router,
  this_pc: Monitor,
  computer: Laptop,
  phone: Smartphone,
  tv: Tv,
  console: Gamepad2,
  printer: Printer,
  camera: Camera,
  speaker: Speaker,
  iot: Lightbulb,
  nas: HardDrive,
  unknown: CircleHelp,
};

const RISK_TONE = { high: "danger", medium: "warning", info: "neutral" } as const;

/** The last scan, kept while the app runs (switching tabs does not lose it). */
let lastScan: DeviceScan | null = null;
let lastPorts: Record<string, OpenPort[]> = {};

function title(d: NetDevice, t: (k: I18nKey) => string): string {
  return d.name ?? d.hostname ?? d.vendor ?? t(`devices.kind.${d.kind}` as I18nKey);
}

export function DevicesTab() {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [scan, setScan] = useState<DeviceScan | null>(lastScan);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [ports, setPorts] = useState<Record<string, OpenPort[]>>(lastPorts);

  const run = async () => {
    setBusy(true);
    setError(null);
    try {
      const s = await backend.devicesScan();
      lastScan = s;
      lastPorts = {};
      setPorts({});
      setScan(s);
    } catch (e) {
      const m = e instanceof Error ? e.message : String(e);
      setError(m === "no_network" ? t("devices.error.no_network") : errorText(e));
    } finally {
      setBusy(false);
    }
  };

  // What Luli reads: no MAC addresses, only what helps judge the network.
  const luliData = useMemo(() => {
    if (!scan) return null;
    return [
      `Devices on ${scan.subnet} (${scan.devices.length}, ${scan.new_count} new since earlier scans):`,
      ...scan.devices.map((d) => {
        const p = ports[d.ip];
        const open = p
          ? ` | open ports: ${p.length ? p.map((x) => `${x.port}/${x.service}`).join(", ") : "none"}`
          : "";
        return `- ${d.kind} | ${d.name ?? d.hostname ?? "?"} | ${d.vendor ?? "?"} | ${d.ip}${d.private_mac ? " | random MAC" : ""}${d.new ? " | NEW" : ""}${d.services.length ? ` | ${d.services.slice(0, 6).join(" ")}` : ""}${open}`;
      }),
    ].join("\n");
  }, [scan, ports]);
  usePageData("network.devices", luliData, true);

  return (
    <>
      <LuliCard
        asks={[
          { label: "luli.ask.devices.strange", data: ["network.devices"] },
          { label: "luli.ask.devices.safe", data: ["network.devices"] },
        ]}
      />
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4" style={rise(1)}>
        <CardTitle help={t("devices.help")} className="mb-0">
          <Radar className="size-4" />
          {t("devices.title")}
        </CardTitle>
        {scan ? (
          <span className="text-xs text-muted-foreground">
            {t("devices.summary", {
              count: scan.devices.length,
              subnet: scan.subnet,
              secs: (scan.elapsed_ms / 1000).toFixed(1),
            })}
          </span>
        ) : null}
        {scan && scan.new_count > 0 ? (
          <Badge tone="warning">{t("devices.new_count", { n: scan.new_count })}</Badge>
        ) : null}
        <Button variant="primary" className="ml-auto" disabled={busy || !inTauri} onClick={run}>
          {busy ? <Loader2 className="size-4 animate-spin" /> : <Radar className="size-4" />}
          {busy ? t("devices.scanning") : scan ? t("devices.rescan") : t("devices.scan")}
        </Button>
      </Card>
      {error ? <p className="text-xs text-destructive">{error}</p> : null}
      {scan?.first_scan ? (
        <p className="text-xs text-muted-foreground">{t("devices.first_scan")}</p>
      ) : null}

      {!scan ? (
        <Card className="animate-rise" style={rise(2)}>
          <EmptyState
            icon={<Radar className="size-5" />}
            title={t("devices.empty.title")}
            body={t("devices.empty.body")}
          />
        </Card>
      ) : (
        <div className="grid grid-cols-1 gap-3 md:grid-cols-2 2xl:grid-cols-3">
          {scan.devices.map((d, i) => (
            <DeviceCard
              key={d.mac}
              device={d}
              index={i}
              ports={ports[d.ip]}
              onPorts={(p) => {
                lastPorts = { ...lastPorts, [d.ip]: p };
                setPorts(lastPorts);
              }}
              onKnown={() => {
                if (!lastScan) return;
                lastScan = {
                  ...lastScan,
                  new_count: Math.max(0, lastScan.new_count - 1),
                  devices: lastScan.devices.map((x) =>
                    x.mac === d.mac ? { ...x, new: false, known: true } : x,
                  ),
                };
                setScan(lastScan);
              }}
            />
          ))}
        </div>
      )}
    </>
  );
}

function DeviceCard({
  device: d,
  index,
  ports,
  onPorts,
  onKnown,
}: {
  device: NetDevice;
  index: number;
  ports: OpenPort[] | undefined;
  onPorts: (p: OpenPort[]) => void;
  onKnown: () => void;
}) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const Icon = ICONS[d.kind];
  const sub = [t(`devices.kind.${d.kind}` as I18nKey), d.vendor, d.model]
    .filter(Boolean)
    .join(" · ");

  const check = async () => {
    setBusy(true);
    setErr(null);
    try {
      onPorts(await backend.devicesPorts(d.ip));
    } catch (e) {
      setErr(errorText(e));
    } finally {
      setBusy(false);
    }
  };
  const known = async () => {
    try {
      await backend.devicesKnown(d.mac, true);
      onKnown();
    } catch (e) {
      setErr(errorText(e));
    }
  };

  return (
    <Card
      className={cn("animate-rise flex flex-col gap-3", d.new && "border-warning/50")}
      style={rise(Math.min(index + 2, 8))}
    >
      <div className="flex items-start gap-3">
        <span
          className={cn(
            "flex size-10 shrink-0 items-center justify-center rounded-lg bg-foreground/6 text-muted-foreground",
            (d.kind === "router" || d.kind === "this_pc") && "bg-primary/12 text-primary",
          )}
          aria-hidden
        >
          <Icon className="size-5" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-center gap-1.5">
            <span className="truncate font-display text-[14px] font-semibold tracking-wide">
              {title(d, t)}
            </span>
            {d.kind === "this_pc" ? (
              <Badge tone="success">{t("devices.badge.this_pc")}</Badge>
            ) : null}
            {d.new ? <Badge tone="warning">{t("devices.badge.new")}</Badge> : null}
          </p>
          <p className="truncate text-xs text-muted-foreground">{sub}</p>
        </div>
      </div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11px]">
        <dt className="text-muted-foreground">IP</dt>
        <dd className="font-mono">{d.ip}</dd>
        <dt className="text-muted-foreground">MAC</dt>
        <dd className="flex items-center gap-1.5 font-mono">
          {d.mac}
          {d.private_mac ? (
            <span className="font-sans text-[10px] text-muted-foreground">
              ({t("devices.private_mac")})
            </span>
          ) : null}
        </dd>
        {d.hostname && d.hostname !== title(d, t) ? (
          <>
            <dt className="text-muted-foreground">{t("devices.hostname")}</dt>
            <dd className="truncate font-mono">{d.hostname}</dd>
          </>
        ) : null}
      </dl>
      {d.services.length > 0 ? (
        <div className="flex flex-wrap gap-1">
          {d.services.slice(0, 5).map((s) => (
            <span
              key={s}
              className="rounded bg-foreground/5 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
            >
              {s}
            </span>
          ))}
        </div>
      ) : null}

      {ports ? (
        ports.length === 0 ? (
          <p className="text-[11px] text-success">{t("devices.ports.none")}</p>
        ) : (
          <ul className="space-y-1.5">
            {ports.map((p) => (
              <li key={p.port} className="text-[11px]">
                <span className="flex items-center gap-1.5">
                  <Badge tone={RISK_TONE[p.risk]}>
                    {p.port} {p.service}
                  </Badge>
                  {p.risk === "high" ? <ShieldAlert className="size-3.5 text-destructive" /> : null}
                </span>
                <span className="mt-0.5 block text-muted-foreground">
                  {t(`devices.port.${p.service}` as I18nKey)}
                </span>
              </li>
            ))}
          </ul>
        )
      ) : null}
      {err ? <p className="text-[11px] text-destructive">{err}</p> : null}

      <div className="mt-auto flex flex-wrap gap-2">
        {d.kind !== "this_pc" ? (
          <Button size="sm" variant="outline" disabled={busy} onClick={check}>
            {busy ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <ShieldAlert className="size-3.5" />
            )}
            {ports ? t("devices.ports.recheck") : t("devices.ports.check")}
          </Button>
        ) : null}
        {d.new ? (
          <Button size="sm" variant="ghost" onClick={known}>
            {t("devices.known")}
          </Button>
        ) : null}
      </div>
    </Card>
  );
}
