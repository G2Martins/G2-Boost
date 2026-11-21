import {
  Check,
  CircleDashed,
  ExternalLink,
  Loader2,
  MinusCircle,
  Play,
  RotateCcw,
  Stethoscope,
} from "lucide-react";
import { type ReactNode, useEffect, useRef, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { Badge, Button, Card, CardTitle, selectClass } from "@/components/ui/primitives";
import {
  backend,
  type DeviceScan,
  type DnsRankEntry,
  inTauri,
  type NetDevice,
  type NetDiagPc,
  type NetUsage,
  type WifiView,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { usePageData } from "@/lib/luli";
import { type Progress, runSpeedTest, type SpeedResult } from "@/lib/speedtest";
import { cn, formatBytes, formatMbps, formatMs } from "@/lib/utils";

/**
 * Rede › Diagnóstico: "my internet is slow" in seven steps. Steps 1 to 5 only read; step 6
 * lists every fix in three groups and waits for the user's answer; step 7 applies only the
 * fixes the app can make (each with its own backup in the core), measures again and shows
 * before and after. Router and device fixes are steps for the user, never automated.
 */

type StepId = "speed" | "pc" | "wifi" | "devices" | "nearby" | "fixes" | "apply";
type StepState = "pending" | "running" | "done" | "skipped";
const STEPS: StepId[] = ["speed", "pc", "wifi", "devices", "nearby", "fixes", "apply"];
const IDLE: Record<StepId, StepState> = {
  speed: "pending",
  pc: "pending",
  wifi: "pending",
  devices: "pending",
  nearby: "pending",
  fixes: "pending",
  apply: "pending",
};

type Group = "pc" | "router" | "devices";
type Fix = {
  id: string;
  group: Group;
  /** The app makes it (with a backup); otherwise the user does it, with steps. */
  auto: boolean;
  title: string;
  body: string;
};
type Answer = "all" | "choose" | "later";

/** Plan speed is a display aid only (the percentage): kept in this viewer's webview. */
const PLAN_KEY = "g2boost.netdiag.plan";
function loadPlan(): string {
  try {
    return localStorage.getItem(PLAN_KEY) ?? "";
  } catch {
    return "";
  }
}
function savePlan(v: string) {
  try {
    localStorage.setItem(PLAN_KEY, v);
  } catch {
    /* private mode: the field still works for this run */
  }
}

/** An app counts as "heavy" from 1 MB/s (8 Mbps) either way. */
const HEAVY_BPS = 1_000_000;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

export function DiagnosisTab() {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [plan, setPlan] = useState(loadPlan);
  const [steps, setSteps] = useState(IDLE);
  const [live, setLive] = useState<Progress | null>(null);
  const [before, setBefore] = useState<SpeedResult | null>(null);
  const [after, setAfter] = useState<SpeedResult | null>(null);
  const [pc, setPc] = useState<NetDiagPc | null>(null);
  const [rank, setRank] = useState<DnsRankEntry[]>([]);
  const [usage, setUsage] = useState<NetUsage | null>(null);
  const [wifi, setWifi] = useState<WifiView | null>(null);
  const [scan, setScan] = useState<DeviceScan | null>(null);
  const [verdict, setVerdict] = useState<Record<string, "mine" | "unknown">>({});
  const [answer, setAnswer] = useState<Answer | null>(null);
  const [chosen, setChosen] = useState<Record<string, boolean>>({});
  const [applied, setApplied] = useState<Record<string, string>>({});
  const [undone, setUndone] = useState<Record<string, boolean>>({});
  const [error, setError] = useState<string | null>(null);
  const [routerMsg, setRouterMsg] = useState<string | null>(null);
  const abort = useRef<AbortController | null>(null);
  // The per-app ETW session of step 2 is open (closed by hand if the tab goes away meanwhile).
  const etw = useRef(false);
  // Leaving the tab ends the run: back on it, a new one would race this one (two speed tests at
  // once measure each other). The core calls have no cancel; the run stops before its next step.
  useEffect(
    () => () => {
      abort.current?.abort();
      if (etw.current) void backend.netUsageStop();
    },
    [],
  );

  const planMbps = Number(plan) > 0 ? Number(plan) : 0;
  const busy = Object.values(steps).includes("running");
  const mark = (id: StepId, s: StepState) => setSteps((x) => ({ ...x, [id]: s }));

  const speedTest = async () => {
    abort.current = new AbortController();
    try {
      return await runSpeedTest(setLive, abort.current.signal);
    } finally {
      setLive(null);
    }
  };

  const start = async () => {
    const live = <T,>(v: T): T => {
      if (abort.current?.signal.aborted) throw new DOMException("left the tab", "AbortError");
      return v;
    };
    savePlan(plan);
    setSteps({ ...IDLE, speed: "running" });
    for (const f of [setBefore, setAfter, setPc, setUsage, setWifi, setScan, setAnswer]) f(null);
    setRank([]);
    setApplied({});
    setUndone({});
    setChosen({});
    setError(null);
    try {
      setBefore(live(await speedTest()));
      mark("speed", "done");

      mark("pc", "running");
      // Per-app rates need two samples of the ETW session a few seconds apart.
      etw.current = true;
      const [p, r] = live(
        await Promise.all([backend.netdiagPc(), backend.dnsRank(4), backend.netUsage()]),
      );
      live(await sleep(3000));
      setUsage(live(await backend.netUsage()));
      etw.current = false;
      void backend.netUsageStop();
      setPc(p);
      setRank(r ?? []);
      mark("pc", "done");

      if (p?.adapter?.kind === "wifi") {
        mark("wifi", "running");
        setWifi(live(await backend.wifiReport(true)));
        mark("wifi", "done");
        mark("nearby", "done");
      } else {
        mark("wifi", "skipped");
        mark("nearby", "skipped");
      }

      mark("devices", "running");
      setScan(live(await backend.devicesScan()));
      mark("devices", "done");
      mark("fixes", "done");
    } catch (e) {
      if (abort.current?.signal.aborted) return; // the tab is gone
      if (etw.current) {
        etw.current = false;
        void backend.netUsageStop();
      }
      setError(errorText(e));
      setSteps((x) => {
        const out = { ...x };
        for (const id of STEPS) if (out[id] === "running") out[id] = "pending";
        return out;
      });
    }
  };

  /* ---------- derived ---------- */
  const adapter = pc?.adapter ?? null;
  const fastest = rank.find((r) => r.median_ms !== null) ?? null;
  const currentDns = rank.find((r) => r.id === adapter?.provider) ?? null;
  const heavyApps = (usage?.processes ?? []).filter(
    (p) => (p.down_bps ?? 0) >= HEAVY_BPS || (p.up_bps ?? 0) >= HEAVY_BPS,
  );
  const myChannel = wifi?.status?.channel ?? null;
  const sharing = myChannel
    ? (wifi?.channels.find((c) => c.channel === myChannel)?.networks ?? 1) - 1
    : 0;
  const strangers = (scan?.devices ?? []).filter((d) => verdict[d.mac] === "unknown");

  // Cheap and built from this render's state: no memo.
  const fixes = ((): Fix[] => {
    if (steps.fixes !== "done") return [];
    const out: Fix[] = [];
    const add = (
      id: string,
      group: Group,
      auto: boolean,
      key: string,
      p: Record<string, string | number> = {},
    ) =>
      out.push({
        id,
        group,
        auto,
        title: t(`netdiag.fix.${key}.title` as I18nKey, p),
        body: t(`netdiag.fix.${key}.body` as I18nKey, p),
      });
    const np = pc?.nic_power;
    if (np && np.on.length > 0)
      add("nic_power", "pc", true, "nic_power", { adapter: np.name, modes: np.on.join(", ") });
    if (adapter && fastest?.median_ms != null && fastest.id !== adapter.provider) {
      const cur = currentDns?.median_ms;
      if (cur == null || cur - fastest.median_ms >= 5)
        add("dns", "pc", true, "dns", {
          name: fastest.name,
          fast: Math.round(fastest.median_ms),
          current: cur == null ? t("netdiag.fix.dns.current_auto") : `${Math.round(cur)} ms`,
        });
    }
    if (pc && pc.vpns.length > 0) add("vpn", "pc", false, "vpn", { names: pc.vpns.join(", ") });
    if (heavyApps.length > 0)
      add("apps", "pc", false, "apps", { apps: heavyApps.map((a) => a.process).join(", ") });
    const st = wifi?.status;
    if (st && wifi?.issues.some((i) => i.kind === "band24_with5_available"))
      add("band", "pc", false, "band");
    if (st && st.signal_quality < 50)
      add("signal", "pc", false, "signal", { pct: st.signal_quality });
    if (
      adapter?.kind === "ethernet" &&
      planMbps > 0 &&
      adapter.link_mbps > 0 &&
      adapter.link_mbps < planMbps
    )
      add("link", "pc", false, "link", { link: adapter.link_mbps, plan: planMbps });

    if (myChannel && wifi?.best_channel && wifi.best_channel !== myChannel && sharing >= 2)
      add("channel", "router", false, "channel", {
        current: myChannel,
        best: wifi.best_channel,
        count: sharing,
      });
    if (before && ["C", "D", "F"].includes(before.grade))
      add("bufferbloat", "router", false, "bufferbloat", {
        idle: Math.round(before.latencyMs),
        loaded: Math.round(Math.max(before.loadedDownMs, before.loadedUpMs)),
        grade: before.grade,
      });
    if (wifi?.issues.some((i) => ["open_network", "wep", "tkip", "wpa1"].includes(i.kind)))
      add("security", "router", false, "security");
    if (before && planMbps > 0 && before.downloadMbps < planMbps * 0.5)
      add("isp", "router", false, "isp", {
        got: Math.round(before.downloadMbps),
        plan: planMbps,
        pct: Math.round((100 * before.downloadMbps) / planMbps),
      });
    if (strangers.length > 0) add("password", "router", false, "password");
    for (const d of strangers)
      add(`device:${d.mac}`, "devices", false, "device", {
        name: deviceName(d, t),
        ip: d.ip,
        maker: d.vendor ?? d.manufacturer ?? "—",
      });
    return out;
  })();

  const autoFixes = fixes.filter((f) => f.auto);
  const picked = (f: Fix) => answer === "all" || (answer === "choose" && chosen[f.id]);

  const apply = async (which: Answer) => {
    setAnswer(which);
    if (which === "later") return;
    if (which === "choose") return; // the checkboxes + "do the chosen" button take over
    await run(autoFixes);
  };
  const run = async (list: Fix[]) => {
    mark("apply", "running");
    setError(null);
    const out: Record<string, string> = {};
    for (const f of list) {
      try {
        if (f.id === "nic_power") await backend.nicPowerOff();
        if (f.id === "dns" && adapter && fastest) await backend.dnsApply(adapter.guid, fastest.id);
        out[f.id] = "ok";
      } catch (e) {
        out[f.id] = errorText(e);
      }
      setApplied({ ...out });
    }
    try {
      setAfter(await speedTest());
    } catch (e) {
      setError(errorText(e));
    }
    mark("apply", "done");
  };
  const undo = async (id: "nic_power" | "dns") => {
    try {
      if (id === "nic_power") await backend.nicPowerRestore();
      if (id === "dns" && adapter) await backend.dnsRestore(adapter.guid);
      setUndone((u) => ({ ...u, [id]: true }));
    } catch (e) {
      setError(errorText(e));
    }
  };
  const openRouter = async () => {
    try {
      const ip = await backend.routerOpen();
      setRouterMsg(t("netdiag.router.opened", { ip }));
    } catch (e) {
      setRouterMsg(errorText(e));
    }
  };

  usePageData(
    "network.diagnosis",
    before
      ? [
          `Slow-internet diagnosis: plan ${planMbps || "?"} Mbps, got ${Math.round(before.downloadMbps)} down / ${Math.round(before.uploadMbps)} up, ping ${before.latencyMs.toFixed(1)} ms, jitter ${before.jitterMs.toFixed(1)} ms, bufferbloat ${before.grade}.`,
          adapter
            ? `Connection: ${adapter.kind}, link ${adapter.link_mbps} Mbps, DNS provider ${adapter.provider ?? "automatic"}.`
            : "",
          fixes.length ? `Fixes listed: ${fixes.map((f) => f.id).join(", ")}.` : "",
        ]
          .filter(Boolean)
          .join("\n")
      : null,
  );

  const started = steps.speed !== "pending";
  const pct = before && planMbps > 0 ? Math.round((100 * before.downloadMbps) / planMbps) : null;

  return (
    <div className="grid gap-4">
      <Card className="animate-rise grid gap-4 md:grid-cols-[1fr_auto] md:items-end">
        <div>
          <CardTitle>
            <Stethoscope className="size-4" />
            {t("netdiag.title")}
          </CardTitle>
          <p className="max-w-2xl text-sm text-muted-foreground">{t("netdiag.intro")}</p>
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <label className="grid gap-1 text-xs text-muted-foreground">
            {t("netdiag.plan")}
            <input
              type="number"
              inputMode="numeric"
              min={1}
              max={100000}
              value={plan}
              disabled={busy}
              placeholder="600"
              onChange={(e) => setPlan(e.target.value.replace(/\D/g, "").slice(0, 6))}
              className={cn(selectClass, "num w-28 text-foreground")}
            />
          </label>
          <Button variant="primary" tactile disabled={busy || !inTauri} onClick={start}>
            {busy ? <Loader2 className="size-4 animate-spin" /> : <Play className="size-4" />}
            {t(started ? "netdiag.again" : "netdiag.start")}
          </Button>
        </div>
        <ol className="flex flex-wrap gap-1.5 md:col-span-2" aria-label={t("netdiag.checklist")}>
          {STEPS.map((id, i) => (
            <li
              key={id}
              className={cn(
                "flex items-center gap-1.5 rounded-md border px-2.5 py-1 text-xs transition-colors duration-300",
                steps[id] === "done" && "border-success/30 text-success",
                steps[id] === "running" && "border-primary/60 bg-primary/10 text-foreground",
                steps[id] === "skipped" && "border-border text-muted-foreground/60 line-through",
                steps[id] === "pending" && "border-border text-muted-foreground",
              )}
            >
              <StepIcon state={steps[id]} />
              <span className="num">{i + 1}.</span> {t(`netdiag.step.${id}` as I18nKey)}
            </li>
          ))}
        </ol>
        {error ? <p className="text-xs text-destructive md:col-span-2">{error}</p> : null}
      </Card>

      {live ? (
        <Card className="animate-rise">
          <p role="status" className="flex items-center gap-2 text-sm">
            <Loader2 className="size-4 animate-spin text-primary" />
            {t(`netdiag.phase.${live.phase}` as I18nKey)}
            <span className="num ml-auto text-muted-foreground">
              {live.downloadMbps ? `↓ ${formatMbps(live.downloadMbps, locale)}` : ""}
              {live.uploadMbps ? `  ↑ ${formatMbps(live.uploadMbps, locale)}` : ""}
            </span>
          </p>
        </Card>
      ) : null}

      {before ? (
        <Step n={1} id="speed">
          <Table
            head={[
              t("netdiag.col.what"),
              t("netdiag.col.plan"),
              t("netdiag.col.got"),
              t("netdiag.col.pct"),
            ]}
            rows={[
              [
                t("netdiag.row.download"),
                planMbps ? formatMbps(planMbps, locale) : "—",
                formatMbps(before.downloadMbps, locale),
                pct === null ? "—" : `${pct}%`,
              ],
              [t("netdiag.row.upload"), "—", formatMbps(before.uploadMbps, locale), "—"],
              [t("netdiag.row.ping"), "—", formatMs(before.latencyMs, locale), "—"],
              [t("netdiag.row.jitter"), "—", formatMs(before.jitterMs, locale), "—"],
              [
                t("netdiag.row.loaded"),
                "—",
                `${formatMs(Math.max(before.loadedDownMs, before.loadedUpMs), locale)} (${before.grade})`,
                "—",
              ],
            ]}
          />
          <p className="mt-2 text-xs text-muted-foreground">
            {pct === null
              ? t("netdiag.speed.noplan")
              : t(pct >= 80 ? "netdiag.speed.ok" : "netdiag.speed.low", { pct })}
          </p>
        </Step>
      ) : null}

      {pc ? (
        <Step n={2} id="pc">
          <Table
            head={[t("netdiag.col.item"), t("netdiag.col.state")]}
            rows={[
              [t("netdiag.pc.conn"), connText(pc, wifi, t)],
              [
                t("netdiag.pc.dns"),
                adapter && adapter.dns.length > 0
                  ? `${adapter.dns.join(", ")} (${adapter.provider ?? t("netdiag.pc.dns_auto")})`
                  : "—",
              ],
              [t("netdiag.pc.vpn"), pc.vpns.length ? pc.vpns.join(", ") : t("netdiag.pc.vpn_none")],
              [
                t("netdiag.pc.power"),
                !pc.nic_power
                  ? t("netdiag.pc.power_unknown")
                  : pc.nic_power.on.length
                    ? t("netdiag.pc.power_on", {
                        count: pc.nic_power.on.length,
                        modes: pc.nic_power.on.join(", "),
                      })
                    : t("netdiag.pc.power_off"),
              ],
            ]}
          />
          <h4 className="mt-4 mb-1 text-xs font-semibold">{t("netdiag.apps.title")}</h4>
          {usage && usage.processes.length > 0 ? (
            <Table
              head={[t("netdiag.col.app"), t("netdiag.col.use"), t("netdiag.col.doing")]}
              rows={[...usage.processes]
                .sort(
                  (a, b) =>
                    (b.down_bps ?? 0) + (b.up_bps ?? 0) - ((a.down_bps ?? 0) + (a.up_bps ?? 0)) ||
                    b.established - a.established,
                )
                .slice(0, 10)
                .map((p) => [
                  p.process,
                  p.down_bps === null
                    ? "—"
                    : `↓ ${formatBytes(p.down_bps, locale)}/s · ↑ ${formatBytes(p.up_bps ?? 0, locale)}/s`,
                  doing(p, t),
                ])}
            />
          ) : (
            <p className="text-xs text-muted-foreground">{t("netdiag.apps.none")}</p>
          )}
          {usage && !usage.bytes_available ? (
            <p className="mt-1 text-[11px] text-muted-foreground">{t("netdiag.apps.no_bytes")}</p>
          ) : null}
        </Step>
      ) : null}

      {steps.wifi === "skipped" ? (
        <Step n={3} id="wifi">
          <p className="text-sm text-muted-foreground">
            {adapter ? t("netdiag.wifi.cable") : t("netdiag.pc.offline")}
          </p>
        </Step>
      ) : wifi ? (
        <Step n={3} id="wifi">
          {wifi.status ? (
            <Table
              head={[
                t("netdiag.wifi.network"),
                t("netdiag.wifi.signal"),
                t("netdiag.wifi.channel"),
                t("netdiag.wifi.band"),
              ]}
              rows={[
                [
                  wifi.status.ssid || t("netdiag.near.hidden"),
                  `${wifi.status.signal_quality}%${wifi.status.rssi_dbm !== null ? ` (${wifi.status.rssi_dbm} dBm)` : ""}`,
                  wifi.status.channel ? String(wifi.status.channel) : "—",
                  bandText(wifi.status.band, t),
                ],
              ]}
            />
          ) : (
            <p className="text-sm text-muted-foreground">{wifiError(wifi.error, t)}</p>
          )}
        </Step>
      ) : null}

      {scan ? (
        <Step n={4} id="devices">
          <p className="mb-2 text-xs text-muted-foreground">
            {t("netdiag.dev.count", { count: scan.devices.length })} {t("netdiag.dev.ask")}
          </p>
          <Table
            head={[t("netdiag.col.name"), t("netdiag.col.type"), t("netdiag.col.maker"), "IP", ""]}
            rows={scan.devices.map((d) => [
              deviceName(d, t),
              t(`devices.kind.${d.kind}` as I18nKey),
              d.vendor ?? d.manufacturer ?? (d.private_mac ? t("netdiag.dev.private") : "—"),
              d.ip,
              <DeviceVerdict
                key={d.mac}
                d={d}
                verdict={verdict[d.mac]}
                onMine={() => {
                  setVerdict((v) => ({ ...v, [d.mac]: "mine" }));
                  void backend.devicesKnown(d.mac, true);
                }}
                onUnknown={() => setVerdict((v) => ({ ...v, [d.mac]: "unknown" }))}
              />,
            ])}
          />
        </Step>
      ) : null}

      {steps.nearby === "done" && wifi && wifi.networks.length > 0 ? (
        <Step n={5} id="nearby">
          <Table
            head={[
              t("netdiag.wifi.network"),
              t("netdiag.wifi.channel"),
              t("netdiag.wifi.signal"),
              t("netdiag.wifi.band"),
            ]}
            rows={[...wifi.networks]
              .sort((a, b) => b.signal_quality - a.signal_quality)
              .slice(0, 15)
              .map((n) => [
                <span key={n.bssid} className={cn(n.connected && "font-semibold text-primary")}>
                  {n.ssid || t("netdiag.near.hidden")}
                </span>,
                n.channel ? String(n.channel) : "—",
                `${n.signal_quality}%`,
                bandText(n.band, t),
              ])}
          />
          {myChannel ? (
            <p className="mt-2 text-xs text-muted-foreground">
              {t("netdiag.near.share", {
                count: sharing,
                channel: myChannel,
                best: wifi.best_channel ?? myChannel,
              })}
            </p>
          ) : null}
        </Step>
      ) : null}

      {steps.fixes === "done" ? (
        <Step n={6} id="fixes">
          {fixes.length === 0 ? (
            <p className="text-sm text-success">{t("netdiag.fix.none")}</p>
          ) : (
            <>
              {(["pc", "router", "devices"] as Group[]).map((g) => {
                const list = fixes.filter((f) => f.group === g);
                if (list.length === 0) return null;
                return (
                  <section key={g} className="mb-4">
                    <h4 className="mb-2 flex items-center gap-2 text-sm font-semibold">
                      {t(`netdiag.group.${g}` as I18nKey)}
                      <Badge tone="neutral">
                        {t("netdiag.group.count", { count: list.length })}
                      </Badge>
                      {g === "router" ? (
                        <Button
                          size="sm"
                          variant="outline"
                          className="ml-auto"
                          onClick={openRouter}
                        >
                          <ExternalLink className="size-3.5" />
                          {t("netdiag.router.open")}
                        </Button>
                      ) : null}
                    </h4>
                    <ul className="grid gap-2">
                      {list.map((f) => (
                        <li
                          key={f.id}
                          className="flex items-start gap-3 rounded-md border border-border px-3 py-2"
                        >
                          {answer === "choose" && f.auto ? (
                            <input
                              type="checkbox"
                              className="mt-1 accent-primary"
                              checked={!!chosen[f.id]}
                              onChange={(e) =>
                                setChosen((c) => ({ ...c, [f.id]: e.target.checked }))
                              }
                              aria-label={f.title}
                            />
                          ) : null}
                          <div className="min-w-0 flex-1">
                            <p className="text-sm font-medium">{f.title}</p>
                            <p className="text-xs text-muted-foreground">{f.body}</p>
                          </div>
                          <Badge tone={f.auto ? "accent" : "neutral"}>
                            {t(f.auto ? "netdiag.fix.auto" : "netdiag.fix.manual")}
                          </Badge>
                        </li>
                      ))}
                    </ul>
                    {g === "router" ? (
                      <p className="mt-2 text-[11px] text-muted-foreground">
                        {routerMsg ?? t("netdiag.router.note")}
                      </p>
                    ) : null}
                  </section>
                );
              })}
              {answer === null ? (
                <div className="flex flex-wrap items-center gap-2 border-t border-border pt-3">
                  <span className="text-sm font-semibold">{t("netdiag.ask.title")}</span>
                  <Button variant="primary" size="sm" disabled={busy} onClick={() => apply("all")}>
                    {t("netdiag.ask.all")}
                  </Button>
                  <Button
                    variant="outline"
                    size="sm"
                    disabled={busy}
                    onClick={() => apply("choose")}
                  >
                    {t("netdiag.ask.choose")}
                  </Button>
                  <Button variant="ghost" size="sm" disabled={busy} onClick={() => apply("later")}>
                    {t("netdiag.ask.later")}
                  </Button>
                  {pc && !pc.elevated && autoFixes.length > 0 ? (
                    <span className="text-xs text-warning">{t("netdiag.admin")}</span>
                  ) : null}
                </div>
              ) : answer === "later" ? (
                <p className="text-sm text-muted-foreground">{t("netdiag.ask.later_done")}</p>
              ) : answer === "choose" && steps.apply === "pending" ? (
                <Button
                  variant="primary"
                  size="sm"
                  disabled={busy}
                  onClick={() => run(autoFixes.filter((f) => chosen[f.id]))}
                >
                  {t("netdiag.ask.go_chosen")}
                </Button>
              ) : null}
            </>
          )}
        </Step>
      ) : null}

      {answer && answer !== "later" && (steps.apply !== "pending" || answer === "all") ? (
        <Step n={7} id="apply">
          {autoFixes.some(picked) ? (
            <>
              <h4 className="mb-1 text-xs font-semibold">{t("netdiag.apply.before")}</h4>
              <Table
                head={[t("netdiag.col.setting"), t("netdiag.col.now"), t("netdiag.col.new"), ""]}
                rows={autoFixes
                  .filter(picked)
                  .map((f) => [
                    f.title,
                    f.id === "nic_power"
                      ? t("netdiag.apply.nic_now", { modes: pc?.nic_power?.on.join(", ") ?? "" })
                      : (adapter?.dns.join(", ") ?? "—"),
                    f.id === "nic_power"
                      ? t("netdiag.apply.nic_new")
                      : fastest
                        ? `${fastest.primary}, ${fastest.secondary}`
                        : "—",
                    <ApplyState
                      key={f.id}
                      state={applied[f.id]}
                      undone={!!undone[f.id]}
                      onUndo={
                        steps.apply === "done" ? () => undo(f.id as "nic_power" | "dns") : undefined
                      }
                    />,
                  ])}
              />
            </>
          ) : (
            <p className="text-sm text-muted-foreground">{t("netdiag.nothing_auto")}</p>
          )}
          {steps.apply === "running" && !live ? (
            <p role="status" className="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
              <Loader2 className="size-3.5 animate-spin" />
              {t("netdiag.apply.running")}
            </p>
          ) : null}
          {before && after ? (
            <>
              <h4 className="mt-4 mb-1 text-xs font-semibold">{t("netdiag.apply.compare")}</h4>
              <Table
                head={[t("netdiag.col.what"), t("netdiag.col.before"), t("netdiag.col.after")]}
                rows={[
                  [
                    t("netdiag.row.download"),
                    formatMbps(before.downloadMbps, locale),
                    formatMbps(after.downloadMbps, locale),
                  ],
                  [
                    t("netdiag.row.upload"),
                    formatMbps(before.uploadMbps, locale),
                    formatMbps(after.uploadMbps, locale),
                  ],
                  [
                    t("netdiag.row.ping"),
                    formatMs(before.latencyMs, locale),
                    formatMs(after.latencyMs, locale),
                  ],
                  [
                    t("netdiag.row.jitter"),
                    formatMs(before.jitterMs, locale),
                    formatMs(after.jitterMs, locale),
                  ],
                  [t("netdiag.row.loaded"), before.grade, after.grade],
                ]}
              />
            </>
          ) : null}
        </Step>
      ) : null}
    </div>
  );
}

/* ---------------- pieces ---------------- */

function Step({ n, id, children }: { n: number; id: StepId; children: ReactNode }) {
  const { t } = useI18n();
  return (
    <Card className="animate-rise">
      <CardTitle>
        <span className="num grid size-5 place-items-center rounded-full bg-success/15 text-[11px] text-success">
          {n}
        </span>
        {t(`netdiag.step.${id}` as I18nKey)}
      </CardTitle>
      {children}
    </Card>
  );
}

function StepIcon({ state }: { state: StepState }) {
  if (state === "done") return <Check className="size-3.5" aria-hidden />;
  if (state === "running") return <Loader2 className="size-3.5 animate-spin" aria-hidden />;
  if (state === "skipped") return <MinusCircle className="size-3.5" aria-hidden />;
  return <CircleDashed className="size-3.5" aria-hidden />;
}

function Table({ head, rows }: { head: string[]; rows: ReactNode[][] }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-xs">
        <thead className="text-muted-foreground">
          <tr className="border-b border-border">
            {head.map((h, i) => (
              <th
                key={h}
                className={cn("py-2 pr-3 font-medium", i === 0 ? "text-left" : "text-right")}
              >
                {h}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((r, ri) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: rows are rebuilt as a whole, never reordered in place
            <tr key={ri} className="border-b border-border/60 last:border-0">
              {r.map((c, ci) => {
                const cls =
                  ci === 0 ? "num py-1.5 pr-3 text-left font-medium" : "num py-1.5 pr-3 text-right";
                return (
                  // biome-ignore lint/suspicious/noArrayIndexKey: fixed columns
                  <td key={ci} className={cls}>
                    {c}
                  </td>
                );
              })}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function DeviceVerdict({
  d,
  verdict,
  onMine,
  onUnknown,
}: {
  d: NetDevice;
  verdict: "mine" | "unknown" | undefined;
  onMine: () => void;
  onUnknown: () => void;
}) {
  const { t } = useI18n();
  if (d.kind === "router") return <Badge tone="neutral">{t("netdiag.dev.router")}</Badge>;
  if (d.kind === "this_pc") return <Badge tone="neutral">{t("netdiag.dev.this_pc")}</Badge>;
  if (verdict === "mine" || (d.known && verdict !== "unknown"))
    return <Badge tone="success">{t("netdiag.dev.ok")}</Badge>;
  if (verdict === "unknown") return <Badge tone="danger">{t("netdiag.dev.flag")}</Badge>;
  return (
    <span className="inline-flex gap-1">
      <Button size="sm" variant="outline" onClick={onMine}>
        {t("netdiag.dev.mine")}
      </Button>
      <Button size="sm" variant="ghost" onClick={onUnknown}>
        {t("netdiag.dev.unknown")}
      </Button>
    </span>
  );
}

function ApplyState({
  state,
  undone,
  onUndo,
}: {
  state?: string;
  undone: boolean;
  onUndo?: () => void;
}) {
  const { t } = useI18n();
  if (state === undefined) return <Loader2 className="ml-auto size-3.5 animate-spin" aria-hidden />;
  if (state !== "ok")
    return <span className="text-destructive">{t("netdiag.apply.failed", { error: state })}</span>;
  if (undone) return <span className="text-muted-foreground">{t("netdiag.apply.undone")}</span>;
  return (
    <span className="inline-flex items-center gap-2">
      <span className="text-success">{t("netdiag.apply.ok")}</span>
      {onUndo ? (
        <Button size="sm" variant="ghost" onClick={onUndo}>
          <RotateCcw className="size-3.5" />
          {t("netdiag.apply.undo")}
        </Button>
      ) : null}
    </span>
  );
}

type T = (k: I18nKey, p?: Record<string, string | number>) => string;

function deviceName(d: NetDevice, t: T): string {
  return d.name ?? d.hostname ?? d.vendor ?? t(`devices.kind.${d.kind}` as I18nKey);
}

function bandText(b: WifiView["networks"][number]["band"], t: T): string {
  return b ? t(`netdiag.band.${b}` as I18nKey) : "—";
}

function connText(pc: NetDiagPc, wifi: WifiView | null, t: T): string {
  const a = pc.adapter;
  if (!a) return t("netdiag.pc.offline");
  if (a.kind === "ethernet") return t("netdiag.pc.cable", { link: a.link_mbps });
  if (a.kind === "wifi")
    return t("netdiag.pc.wifi", { band: bandText(wifi?.status?.band ?? null, t) });
  return t("netdiag.pc.other", { name: a.description });
}

function wifiError(code: string | null, t: T): string {
  if (code === "location_denied") return t("netdiag.wifi.location");
  if (code === "no_service") return t("netdiag.wifi.service");
  return t("netdiag.wifi.unavailable");
}

/** What a process is doing, from its live rates and sockets. */
function doing(p: NetUsage["processes"][number], t: T): string {
  const down = p.down_bps ?? 0;
  const up = p.up_bps ?? 0;
  if (down >= 100_000 && down >= up) return t("netdiag.apps.down");
  if (up >= 100_000) return t("netdiag.apps.up");
  if (p.established > 0) return t("netdiag.apps.conns", { count: p.established });
  if (p.listening_ports.length > 0) return t("netdiag.apps.listen");
  return t("netdiag.apps.idle");
}
