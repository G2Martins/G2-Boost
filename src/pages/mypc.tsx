import {
  Cpu,
  FileDown,
  Gauge as GaugeIcon,
  HardDrive,
  MemoryStick,
  Monitor,
  RefreshCw,
  ShieldCheck,
  Tv,
} from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { BiosGuideCard } from "@/components/bios-guide";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  LoadingScope,
  MetricRow,
} from "@/components/ui/primitives";
import { backend, type Firmware, inTauri, type Profile, type WinSat } from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

/** "Meu PC": the user-mode subset of what CPU-Z / Speccy show (Docs/references/speccy-cpuz.md). */
export function MyPc({ elevated }: { elevated: boolean }) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [profile, setProfile] = useState<Profile | null>(null);
  const [sat, setSat] = useState<WinSat | null>(null);
  const [fw, setFw] = useState<Firmware | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [satBusy, setSatBusy] = useState(false);
  const started = useRef(false);

  const load = useCallback(async (refresh: boolean) => {
    setBusy(true);
    const [p, w] = await Promise.all([backend.systemProfile(refresh), backend.winsatLast()]);
    if (p) setProfile(p);
    if (w) setSat(w);
    const f = await backend.firmwareInfo();
    if (f) setFw(f);
    setBusy(false);
  }, []);
  useEffect(() => {
    if (!started.current) {
      started.current = true;
      void load(false);
    }
  }, [load]);

  const saveReport = async () => {
    setMsg(null);
    try {
      const path = await backend.reportSave();
      setMsg(t("mypc.report_saved", { path }));
    } catch (e) {
      setMsg(errorText(e));
    }
  };

  const runWinSat = async () => {
    setSatBusy(true);
    setMsg(null);
    try {
      await backend.winsatStart();
      setMsg(t("mypc.winsat.started"));
    } catch (e) {
      setMsg(errorText(e));
    } finally {
      setSatBusy(false);
    }
  };

  const uptime = (secs: number) =>
    t("mypc.uptime_value", { h: Math.floor(secs / 3600), m: Math.floor((secs % 3600) / 60) });
  const yn = (v: boolean | null) => (v === null ? "—" : v ? t("settings.yes") : t("settings.no"));
  const cpu = profile?.cpu_detail;
  const hex = (n: number) => n.toString(16).toUpperCase();
  const dimms = profile?.memory.filter((m) => m.size_bytes > 0) ?? [];
  const configured = dimms.find((m) => m.configured_mts > 0)?.configured_mts ?? 0;
  const spd = dimms[0]?.speed_mts ?? 0;
  const rated = dimms.map((m) => m.rated_mts ?? 0).reduce((a, b) => Math.max(a, b), 0);
  const satState = (s: number) =>
    s === 1
      ? "valid"
      : s === 2
        ? "incoherent"
        : s === 3
          ? "never"
          : s === 4
            ? "invalid"
            : "unknown";

  return (
    <LoadingScope loading={inTauri && !profile}>
      <div className="flex flex-wrap items-center gap-2">
        {msg ? <p className="animate-rise text-xs text-foreground/90">{msg}</p> : null}
        <div className="ml-auto flex gap-2">
          <Button
            variant="outline"
            size="sm"
            disabled={busy || !inTauri}
            onClick={() => load(true)}
          >
            <RefreshCw className={cn("size-4", busy && "animate-spin")} />
            {t("mypc.refresh")}
          </Button>
          <Button variant="primary" size="sm" disabled={!inTauri || !profile} onClick={saveReport}>
            <FileDown className="size-4" />
            {t("mypc.save_report")}
          </Button>
        </div>
      </div>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Card className="animate-rise">
          <CardTitle>
            <Cpu className="size-4" />
            {t("mypc.cpu")}
            <span className="ml-2 font-sans text-xs font-normal text-muted-foreground">
              {profile?.cpu ?? "—"}
            </span>
          </CardTitle>
          <div className="divide-y divide-border">
            <MetricRow
              label={t("mypc.cpu_fms")}
              value={
                cpu
                  ? `${cpu.vendor} · ${hex(cpu.family)} / ${hex(cpu.model)} / ${hex(cpu.stepping)}`
                  : "—"
              }
            />
            <MetricRow
              label={t("mypc.cores")}
              value={
                profile
                  ? t("mypc.cores_value", {
                      phys: profile.cores_physical,
                      logical: profile.cores_logical,
                    })
                  : "—"
              }
            />
            <MetricRow
              label={t("mypc.ht")}
              value={cpu ? yn(cpu.hyperthreading) : "—"}
              ok={cpu?.hyperthreading}
            />
            <MetricRow
              label={t("mypc.virtualization")}
              value={
                fw?.hypervisor_present
                  ? t("mypc.fw.virt.hyperv")
                  : cpu
                    ? yn(cpu.virtualization)
                    : "—"
              }
            />
            <MetricRow label={t("mypc.microcode")} value={cpu?.microcode ?? "—"} />
          </div>
          {cpu && cpu.caches.length > 0 ? (
            <div className="mt-3">
              <p className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                {t("mypc.caches")}
              </p>
              <div className="flex flex-wrap gap-1.5">
                {cpu.caches.map((c) => (
                  <Badge key={`${c.level}-${c.kind}`} tone="neutral">
                    L{c.level} {t(`mypc.cache.${c.kind}` as const)} · {c.count} ×{" "}
                    {formatBytes(c.size_bytes, locale, 0)}
                  </Badge>
                ))}
              </div>
            </div>
          ) : null}
          {cpu && cpu.flags.length > 0 ? (
            <div className="mt-3">
              <p className="mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                {t("mypc.flags")}
              </p>
              <div className="flex flex-wrap gap-1">
                {cpu.flags.map((f) => (
                  <span
                    key={f}
                    className="rounded border border-border px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground"
                  >
                    {f}
                  </span>
                ))}
              </div>
            </div>
          ) : null}
        </Card>

        <Card className="animate-rise" style={rise(1)}>
          <CardTitle>
            <MemoryStick className="size-4" />
            {t("mypc.memory")}
            <span className="ml-2 font-sans text-xs font-normal text-muted-foreground">
              {profile
                ? t("mypc.memory.summary", {
                    total: formatBytes(profile.ram_bytes, locale, 0),
                    used: dimms.length,
                    slots: profile.memory_slots ?? dimms.length,
                  })
                : "—"}
            </span>
          </CardTitle>
          {dimms.length > 0 ? (
            <>
              <p className="num text-sm">
                {t("mypc.memory.speed", { configured: configured || "?", spd: spd || "?" })}
                {rated > 0 ? ` · ${t("mypc.memory.rated", { rated })}` : ""}
              </p>
              {profile?.memory_below_rated ? (
                <p className="mt-2 rounded-md border border-warning/40 bg-warning/10 px-3 py-2 text-xs text-warning">
                  {t("mypc.memory.below_rated")}
                </p>
              ) : null}
              <ul className="mt-3 divide-y divide-border text-xs">
                {profile?.memory.map((m) => (
                  <li
                    key={`${m.slot}-${m.bank}`}
                    className="flex flex-wrap items-center gap-2 py-2"
                  >
                    <span className="font-mono text-muted-foreground">{m.slot || m.bank}</span>
                    {m.size_bytes > 0 ? (
                      <>
                        <span className="num font-semibold">
                          {formatBytes(m.size_bytes, locale, 0)}
                        </span>
                        <Badge tone="neutral">{m.kind}</Badge>
                        {m.form_factor ? <Badge tone="neutral">{m.form_factor}</Badge> : null}
                        <span className="num text-muted-foreground">
                          {m.configured_mts || m.speed_mts} MT/s
                        </span>
                        <span
                          className="min-w-0 flex-1 truncate text-muted-foreground"
                          title={`${m.manufacturer} ${m.part_number}`}
                        >
                          {m.manufacturer} {m.part_number}
                        </span>
                      </>
                    ) : (
                      <span className="text-muted-foreground">{t("mypc.memory.empty_slot")}</span>
                    )}
                  </li>
                ))}
              </ul>
            </>
          ) : (
            <p className="text-xs text-muted-foreground">—</p>
          )}
        </Card>

        <Card className="animate-rise" style={rise(2)}>
          <CardTitle>
            <Monitor className="size-4" />
            {t("mypc.video")}
          </CardTitle>
          <div className="divide-y divide-border">
            {(profile?.gpus ?? []).map((g) => (
              <div key={g.name} className="py-2">
                <div className="flex flex-wrap items-center gap-2 text-sm">
                  <span className="font-semibold">{g.name}</span>
                  <Badge tone="neutral">{formatBytes(g.vram_bytes, locale, 0)}</Badge>
                  {g.pcie ? <Badge tone="neutral">{g.pcie}</Badge> : null}
                  {g.power_limit_w ? (
                    <Badge tone="neutral">{t("mypc.tdp", { w: g.power_limit_w })}</Badge>
                  ) : null}
                </div>
                <p className="num mt-1 text-xs text-muted-foreground">
                  {t("mypc.driver")}: {g.driver ?? "—"}
                </p>
              </div>
            ))}
          </div>
          <p className="mt-3 mb-1.5 flex items-center gap-1.5 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            <Tv className="size-3.5" />
            {t("mypc.monitors")}
          </p>
          <ul className="divide-y divide-border text-sm">
            {(profile?.monitors ?? []).map((m) => (
              <li
                key={`${m.name}-${m.width}x${m.height}-${m.refresh_hz}`}
                className="flex items-center gap-3 py-1.5"
              >
                <span className="min-w-0 flex-1 truncate">{m.name}</span>
                <span className="num text-xs text-muted-foreground">
                  {m.width}×{m.height} @ {Math.round(m.refresh_hz)} Hz
                </span>
              </li>
            ))}
          </ul>
        </Card>

        <div className="flex flex-col gap-4">
          <Card className="animate-rise" style={rise(3)}>
            <CardTitle>
              <GaugeIcon className="size-4" />
              {t("mypc.winsat")}
              <span className="ml-2 font-sans text-xs font-normal text-muted-foreground">
                {sat ? t(`mypc.winsat.state.${satState(sat.state)}` as const) : "—"}
                {sat?.taken ? ` · ${t("mypc.winsat.taken", { when: sat.taken })}` : ""}
              </span>
            </CardTitle>
            <div className="flex flex-wrap items-end gap-4">
              <div>
                <div className="num font-display text-[40px] font-semibold leading-none">
                  {sat?.base?.toFixed(1) ?? "—"}
                </div>
                <div className="text-[10px] uppercase tracking-wider text-muted-foreground">
                  {t("mypc.winsat.base")}
                </div>
              </div>
              <div className="grid flex-1 grid-cols-5 gap-2 text-center">
                {(["cpu", "memory", "graphics", "d3d", "disk"] as const).map((k) => (
                  <div
                    key={k}
                    className="rounded-md border border-border bg-background/60 px-2 py-1.5"
                  >
                    <div className="num font-display text-sm font-semibold">
                      {sat?.[k]?.toFixed(1) ?? "—"}
                    </div>
                    <div className="text-[10px] text-muted-foreground">
                      {t(`mypc.winsat.${k}` as const)}
                    </div>
                  </div>
                ))}
              </div>
            </div>
            <p className="mt-3 text-xs text-muted-foreground">{t("mypc.winsat.hint")}</p>
            <div className="mt-3">
              {elevated ? (
                <Button
                  variant="outline"
                  size="sm"
                  disabled={satBusy || !inTauri}
                  onClick={runWinSat}
                >
                  {satBusy ? t("mypc.winsat.running") : t("mypc.winsat.run")}
                </Button>
              ) : (
                <ElevateButton route="dashboard" />
              )}
            </div>
          </Card>

          <Card className="animate-rise" style={rise(4)}>
            <CardTitle>
              <ShieldCheck className="size-4" />
              {t("mypc.board_bios")}
            </CardTitle>
            <div className="divide-y divide-border">
              <MetricRow label={t("mypc.board")} value={profile?.board ?? "—"} />
              {profile?.system_model && profile.system_model !== profile.board ? (
                <MetricRow label={t("mypc.model")} value={profile.system_model} />
              ) : null}
              <MetricRow
                label={t("mypc.bios")}
                value={
                  profile?.bios
                    ? `${profile.bios.vendor} ${profile.bios.version} (${profile.bios.date})`
                    : "—"
                }
              />
              <MetricRow
                label={t("mypc.type")}
                value={profile ? (profile.laptop ? t("mypc.laptop") : t("mypc.desktop")) : "—"}
              />
              <MetricRow
                label={t("mypc.secure_boot")}
                value={profile ? yn(profile.secure_boot) : "—"}
                ok={profile?.secure_boot ?? undefined}
              />
              <MetricRow
                label={t("mypc.tpm")}
                value={profile ? (profile.tpm ?? t("mypc.tpm.none")) : "—"}
                ok={profile ? profile.tpm !== null : undefined}
              />
              <MetricRow
                label={t("mypc.fw.boot")}
                value={fw?.boot_mode ? t(`mypc.fw.boot.${fw.boot_mode}`) : "—"}
                ok={fw?.boot_mode ? fw.boot_mode === "uefi" : undefined}
              />
              <MetricRow
                label={t("mypc.fw.virt")}
                value={
                  fw
                    ? fw.hypervisor_present
                      ? t("mypc.fw.virt.hyperv")
                      : yn(fw.virtualization_enabled)
                    : "—"
                }
              />
              <MetricRow
                label={t("mypc.fw.rebar")}
                value={
                  fw?.resizable_bar == null
                    ? t("mypc.fw.unknown")
                    : `${yn(fw.resizable_bar)} (${fw.bar1_mb} MB)`
                }
                ok={fw?.resizable_bar ?? undefined}
              />
              <MetricRow
                label={t("mypc.fw.xmp")}
                value={
                  fw?.memory_at_rated_speed == null
                    ? t("mypc.fw.unknown")
                    : yn(fw.memory_at_rated_speed)
                }
                ok={fw?.memory_at_rated_speed ?? undefined}
              />
              <MetricRow
                label={t("mypc.fw.bios_age")}
                value={
                  fw?.bios_age_days != null
                    ? t("mypc.fw.years", { n: (fw.bios_age_days / 365).toFixed(1) })
                    : "—"
                }
              />
              <MetricRow label={t("mypc.fw.fast_startup")} value={fw ? yn(fw.fast_startup) : "—"} />
              <MetricRow label={t("mypc.fw.hibernation")} value={fw ? yn(fw.hibernation) : "—"} />
            </div>
            <p className="mt-2 text-[11px] text-muted-foreground">{t("mypc.fw.note")}</p>
          </Card>
        </div>

        <Card className="animate-rise" style={rise(5)}>
          <CardTitle>
            <HardDrive className="size-4" />
            {t("mypc.disks")}
          </CardTitle>
          {profile && profile.disks.length > 0 ? (
            <ul className="divide-y divide-border text-sm">
              {profile.disks.map((d) => (
                <li key={d.index} className="flex flex-wrap items-center gap-2 py-2">
                  <span className="min-w-0 flex-1 truncate font-medium" title={d.model}>
                    {d.model}
                  </span>
                  <Badge tone="neutral">{t(`mypc.bus.${d.bus}` as const)}</Badge>
                  <Badge
                    className={cn(
                      d.ssd === true
                        ? "bg-success/12 text-success"
                        : d.ssd === false
                          ? "bg-warning/12 text-warning"
                          : "bg-card-hover text-muted-foreground",
                    )}
                  >
                    {d.ssd === true
                      ? t("mypc.ssd")
                      : d.ssd === false
                        ? t("mypc.hdd")
                        : t("mypc.media_unknown")}
                  </Badge>
                  {d.system ? <Badge tone="accent">{t("mypc.system_disk")}</Badge> : null}
                  <span className="num text-xs text-muted-foreground">
                    {formatBytes(d.size_bytes, locale, 0)}
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-xs text-muted-foreground">—</p>
          )}
        </Card>

        <Card className="animate-rise" style={rise(6)}>
          <CardTitle>{t("mypc.system")}</CardTitle>
          <div className="divide-y divide-border">
            <MetricRow label={t("mypc.computer")} value={profile?.computer ?? "—"} />
            <MetricRow label={t("mypc.os")} value={profile?.os ?? "—"} />
            <MetricRow
              label={t("mypc.uptime")}
              value={profile ? uptime(profile.uptime_secs) : "—"}
            />
            <MetricRow
              label={t("settings.elevated")}
              value={profile ? yn(profile.elevated) : "—"}
              ok={profile?.elevated}
            />
          </div>
        </Card>

        <BiosGuideCard elevated={elevated} index={7} />
      </div>
    </LoadingScope>
  );
}
