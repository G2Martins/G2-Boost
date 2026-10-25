import {
  Check as CheckIcon,
  Home,
  Loader2,
  Route as RouteIcon,
  Server,
  TriangleAlert,
  User,
  Wand2,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Badge, Button, Card, CardTitle } from "@/components/ui/primitives";
import { backend, type GameEntry, inTauri } from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { navigate } from "@/lib/nav";
import { analyzeRoute, type RouteAnalysis, type Zone } from "@/lib/route";
import { cn } from "@/lib/utils";

type Sdr = { pops?: Record<string, { desc?: string; relays?: { ipv4: string }[] }> };
type Target = { host: string; label: string };
type Check = {
  key: I18nKey;
  vars?: Record<string, string | number>;
  ok: boolean;
  go?: "boost" | "network";
};

const ZONE_STRIPE: Record<Zone, string> = {
  home: "bg-muted-foreground/60",
  isp: "bg-chart-2",
  internet: "bg-primary",
};

/** Nearest Steam Datagram Relay for a Steam game (the "server" Valve games really talk to). */
async function nearestRelay(appid: string, onStep: (done: number, total: number) => void) {
  const cfg = await backend.fetchJson<Sdr>(
    `https://api.steampowered.com/ISteamApps/GetSDRConfig/v1/?appid=${encodeURIComponent(appid)}`,
  );
  const pops = Object.entries(cfg.pops ?? {}).flatMap(([code, p]) => {
    const ip = p.relays?.[0]?.ipv4;
    return typeof ip === "string" && /^\d{1,3}(\.\d{1,3}){3}$/.test(ip)
      ? [{ code, desc: p.desc ?? code, ip }]
      : [];
  });
  let best: { code: string; desc: string; ip: string; rtt: number } | null = null;
  for (let i = 0; i < pops.length; i += 8) {
    const res = await Promise.all(
      pops.slice(i, i + 8).map(async (p) => {
        try {
          return { ...p, rtt: (await backend.pingHost(p.ip, 2, 900, 60)).stats.avg_ms };
        } catch {
          return { ...p, rtt: null };
        }
      }),
    );
    for (const r of res)
      if (r.rtt !== null && (!best || r.rtt < best.rtt)) best = { ...r, rtt: r.rtt };
    onStep(Math.min(i + 8, pops.length), pops.length);
  }
  return best;
}

/** One game's route: where it connects, the path there hop by hop, and an honest "re-optimize". */
export function RoutePanel({ game }: { game: GameEntry }) {
  const { t } = useI18n();
  const [targets, setTargets] = useState<Target[]>([]);
  const [target, setTarget] = useState("");
  const [relayStep, setRelayStep] = useState<string | null>(null);
  const [route, setRoute] = useState<RouteAnalysis | null>(null);
  const [tracing, setTracing] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [checks, setChecks] = useState<Check[] | null>(null);
  const [checking, setChecking] = useState(false);
  const steamApp = game.id.startsWith("steam:") ? game.id.slice(6) : null;

  useEffect(() => {
    setRoute(null);
    setChecks(null);
    setTargets([]);
    setTarget("");
    if (!game.running) return;
    backend.gameServers(game.id).then((s) => {
      const list = (s ?? []).map((x) => ({ host: x.ip, label: `${x.ip}:${x.port}` }));
      setTargets(list);
      if (list[0]) setTarget(list[0].host);
    });
  }, [game.id, game.running]);

  const findRelay = async () => {
    if (!steamApp) return;
    setRelayStep("0/?");
    try {
      const best = await nearestRelay(steamApp, (d, n) => setRelayStep(`${d}/${n}`));
      if (best) {
        const tgt = {
          host: best.ip,
          label: t("route.relay", { name: best.desc, ms: Math.round(best.rtt) }),
        };
        setTargets((cur) => [tgt, ...cur.filter((x) => x.host !== best.ip)]);
        setTarget(best.ip);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setRelayStep(null);
    }
  };

  const trace = useCallback(
    async (override?: string) => {
      const host = (override ?? target).trim();
      if (!host) return;
      setTracing(true);
      setError(null);
      try {
        setRoute(analyzeRoute(await backend.traceHost(host, 30, 3, 1000)));
      } catch (e) {
        setError(e instanceof Error ? e.message : String(e));
      } finally {
        setTracing(false);
      }
    },
    [target],
  );

  // Dev aid (screenshots): G2BOOST_START_TAB=route + autorun traces to a public resolver.
  // biome-ignore lint/correctness/useExhaustiveDependencies: fires once on mount
  useEffect(() => {
    if (boot.autorun && boot.startTab === "route") {
      setTarget("1.1.1.1");
      void trace("1.1.1.1");
    }
  }, []);

  /** Re-optimize, honestly: what is in our hands (DNS, Wi-Fi, home network, relay choice). */
  const reoptimize = async () => {
    setChecking(true);
    const out: Check[] = [];
    const [dns, wifi] = await Promise.all([backend.dnsRank(4), backend.wifiReport(false)]);
    const fastest = (dns ?? [])
      .filter((d) => d.median_ms !== null)
      .sort((a, b) => (a.median_ms ?? 0) - (b.median_ms ?? 0))[0];
    if (fastest)
      out.push({
        key: "route.check.dns",
        vars: { name: fastest.name, ms: Math.round(fastest.median_ms ?? 0) },
        ok: false,
        go: "boost",
      });
    if (wifi?.status) {
      const bad = wifi.issues.filter(
        (i) => i.kind === "weak_signal" || i.kind === "band24_with5_available",
      );
      out.push(
        bad.length
          ? { key: "route.check.wifi_bad", ok: false, go: "network" }
          : { key: "route.check.wifi_ok", ok: true },
      );
    } else out.push({ key: "route.check.cable", ok: true });
    const home = route?.nodes.find((n) => n.zone === "home");
    if (home) {
      out.push(
        home.loss > 0 || (home.avg ?? 0) > 5
          ? {
              key: "route.check.home_bad",
              vars: { ms: Math.round(home.avg ?? 0), loss: home.loss },
              ok: false,
            }
          : { key: "route.check.home_ok", vars: { ms: Math.round(home.avg ?? 0) }, ok: true },
      );
    }
    if (route?.worst !== null && route?.worst !== undefined) {
      const zone = route.nodes[route.worst]?.zone ?? "internet";
      out.push({ key: `route.check.worst_${zone}` as I18nKey, ok: false });
    }
    out.push({ key: "route.check.bufferbloat", ok: true, go: "network" });
    setChecks(out);
    setChecking(false);
  };

  const verdict = (() => {
    if (!route) return null;
    if (route.lossFrom !== null)
      return t("route.verdict.loss", { hop: route.nodes[route.lossFrom]?.ttl ?? 0 });
    if (route.worst === null) return t("route.verdict.clean");
    const n = route.nodes[route.worst];
    return t(`route.verdict.worst_${n?.zone ?? "internet"}` as I18nKey, {
      hop: n?.ttl ?? 0,
      ms: Math.round(n?.delta ?? 0),
    });
  })();

  return (
    <Card className="animate-rise">
      <CardTitle help={t("route.help")}>
        <RouteIcon className="size-4" />
        {t("route.title", { game: game.name })}
      </CardTitle>
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={target}
          onChange={(e) => setTarget(e.target.value.trim().slice(0, 253))}
          list="route-targets"
          placeholder={t("route.target.placeholder")}
          aria-label={t("route.target.placeholder")}
          className="num h-9 min-w-56 flex-1 rounded-md border border-border bg-background px-3 text-xs outline-none focus:border-ring"
        />
        <datalist id="route-targets">
          {targets.map((x) => (
            <option key={x.host} value={x.host}>
              {x.label}
            </option>
          ))}
        </datalist>
        {steamApp ? (
          <Button
            variant="outline"
            size="sm"
            disabled={!!relayStep || !inTauri}
            onClick={findRelay}
          >
            {relayStep ? (
              <Loader2 className="size-3.5 animate-spin" />
            ) : (
              <Server className="size-3.5" />
            )}
            {relayStep ? t("route.relay.finding", { step: relayStep }) : t("route.relay.find")}
          </Button>
        ) : null}
        <Button
          variant="primary"
          size="sm"
          disabled={!target || tracing || !inTauri}
          onClick={() => void trace()}
        >
          {tracing ? (
            <Loader2 className="size-3.5 animate-spin" />
          ) : (
            <RouteIcon className="size-3.5" />
          )}
          {t("route.trace")}
        </Button>
      </div>
      <p className="mt-2 text-[11px] text-muted-foreground">
        {game.running
          ? targets.length
            ? t("route.servers.found", { n: targets.length })
            : t("route.servers.udp")
          : t("route.servers.not_running")}
      </p>
      {error ? <p className="mt-2 text-xs text-destructive">{error}</p> : null}

      {route ? (
        <>
          <div className="mt-4 flex flex-wrap items-center gap-y-3">
            <span className="flex flex-col items-center gap-1 px-1">
              <span className="flex size-10 items-center justify-center rounded-full border border-border-strong">
                <User className="size-4" />
              </span>
              <span className="text-[10px] text-muted-foreground">{t("route.you")}</span>
            </span>
            {route.nodes.map((n, i) => (
              <span key={n.ttl} className="flex items-center">
                <span
                  className={cn(
                    "num mx-1 rounded-md border px-1.5 py-0.5 text-[10px]",
                    i === route.worst
                      ? "border-destructive/60 text-destructive"
                      : "border-border text-muted-foreground",
                  )}
                >
                  {n.delta !== null ? `+${Math.round(n.delta)}ms` : "·"}
                </span>
                <span
                  className={cn(
                    "relative min-w-28 overflow-hidden rounded-md border bg-background/60 py-1.5 pr-2 pl-3",
                    i === route.worst
                      ? "border-destructive/60"
                      : n.zone === "internet" && i === route.nodes.length - 1
                        ? "border-primary/60"
                        : "border-border",
                  )}
                >
                  <span
                    className={cn("absolute inset-y-0 left-0 w-1", ZONE_STRIPE[n.zone])}
                    aria-hidden
                  />
                  <span className="flex items-center gap-1 text-[10px] text-muted-foreground">
                    {n.zone === "home" ? <Home className="size-3" /> : null}
                    {t(`route.zone.${n.zone}` as I18nKey)} · {n.ttl}
                  </span>
                  <span className="num block text-[11px] font-semibold">{n.addr ?? "* * *"}</span>
                  <span className="num block text-[10px] text-muted-foreground">
                    {n.avg !== null ? `${Math.round(n.avg)} ms` : "—"}
                    {n.loss > 0 && n.avg !== null ? ` · ${n.loss}%` : ""}
                  </span>
                </span>
              </span>
            ))}
          </div>
          <p className="mt-4 text-sm">
            {route.total !== null ? (
              <span className="num mr-2 font-display text-lg font-semibold">
                {Math.round(route.total)} ms
              </span>
            ) : null}
            {verdict}
          </p>
          <div className="mt-4 border-t border-border pt-4">
            <Button
              variant="outline"
              size="sm"
              disabled={checking || !inTauri}
              onClick={reoptimize}
            >
              {checking ? (
                <Loader2 className="size-3.5 animate-spin" />
              ) : (
                <Wand2 className="size-3.5" />
              )}
              {t("route.reoptimize")}
            </Button>
            {checks ? (
              <ul className="mt-3 space-y-1.5">
                {checks.map((c) => (
                  <li key={c.key} className="flex flex-wrap items-center gap-2 text-xs">
                    <Badge tone={c.ok ? "success" : "warning"}>
                      {c.ok ? (
                        <CheckIcon className="size-3" />
                      ) : (
                        <TriangleAlert className="size-3" />
                      )}
                    </Badge>
                    <span className="min-w-0 flex-1">{t(c.key, c.vars)}</span>
                    {c.go ? (
                      <Button size="sm" variant="ghost" onClick={() => navigate(c.go ?? "network")}>
                        {t(c.go === "boost" ? "route.go.boost" : "route.go.network")}
                      </Button>
                    ) : null}
                  </li>
                ))}
              </ul>
            ) : null}
          </div>
        </>
      ) : null}
    </Card>
  );
}
