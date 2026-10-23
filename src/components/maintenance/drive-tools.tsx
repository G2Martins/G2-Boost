import {
  ChevronRight,
  Clock,
  HardDrive,
  Layers,
  LifeBuoy,
  type LucideIcon,
  Power,
  Square,
  TriangleAlert,
} from "lucide-react";
import { Fragment, useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, Modal, Skeleton } from "@/components/ui/primitives";
import {
  backend,
  inTauri,
  type Volume,
  type VolumeOp,
  type VolumeOutcome,
  type VolumeProgress,
  type VolumeVerdict,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

type Family = "check" | "optimize";
const FAMILY: Record<Family, VolumeOp[]> = {
  check: ["scan", "spot_fix"],
  optimize: ["analyze", "retrim", "defrag"],
};

const TONE: Record<VolumeVerdict, "success" | "warning" | "danger" | "neutral"> = {
  healthy: "success",
  fixed: "success",
  optimized: "success",
  needs_spot_fix: "warning",
  needs_offline: "warning",
  retry: "warning",
  failed: "danger",
  cancelled: "neutral",
};

const HEALTH_TONE: Record<Volume["health"], "success" | "warning" | "danger" | "neutral"> = {
  healthy: "success",
  warning: "warning",
  unhealthy: "danger",
  unknown: "neutral",
};

type Job = {
  op: VolumeOp;
  volume: string;
  percent: number | null;
  since: number;
  adopted?: boolean;
};

function elapsed(ms: number) {
  const s = Math.max(0, Math.floor(ms / 1000));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/** Method return values in decimal (40001, 43001...), HRESULTs in hex. */
const codeText = (code: number) =>
  code >= 0x80000000 ? `0x${code.toString(16).toUpperCase()}` : String(code);

/** "Verificar disco" + "Otimizar unidades": one row per fixed lettered volume. */
export function DriveTools({
  elevated,
  repairRunning,
  onRunning,
  index,
}: {
  elevated: boolean;
  /** A DISM/SFC/MRT job owns the servicing slot. */
  repairRunning: boolean;
  onRunning: (on: boolean) => void;
  index: number;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [volumes, setVolumes] = useState<Volume[] | null>(null);
  const [history, setHistory] = useState<VolumeOutcome[]>([]);
  const [job, setJob] = useState<Job | null>(null);
  const [confirm, setConfirm] = useState<{ op: VolumeOp; volume: string } | null>(null);
  const [error, setError] = useState<{ op: VolumeOp; volume: string; text: string } | null>(null);
  const [, tick] = useState(0);

  const refresh = async () => {
    const [v, h] = await Promise.all([backend.volumesList(), backend.volumesHistory()]);
    setVolumes(v ?? []);
    if (h) setHistory(h);
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: load once on mount
  useEffect(() => {
    void refresh();
    backend.volumesStatus().then((s) => {
      if (s)
        setJob({
          op: s.op,
          volume: s.volume,
          percent: s.percent,
          since: Date.now(),
          adopted: true,
        });
    });
    let off: (() => void) | null = null;
    backend
      .on<VolumeProgress>("maintenance:volume-progress", (p) =>
        setJob((j) =>
          j && j.volume === p.volume && j.op === p.op ? { ...j, percent: p.percent } : j,
        ),
      )
      .then((u) => {
        off = u;
      });
    return () => off?.();
  }, []);

  useEffect(() => onRunning(job !== null), [job, onRunning]);

  useEffect(() => {
    if (!job) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [job]);

  // Started before this page mounted: nobody awaits it here, so poll for the end.
  const adopted = job?.adopted === true;
  // biome-ignore lint/correctness/useExhaustiveDependencies: refresh is stable enough for a poll
  useEffect(() => {
    if (!adopted) return;
    const id = window.setInterval(async () => {
      if (!(await backend.volumesStatus())) {
        setJob(null);
        await refresh();
      }
    }, 3000);
    return () => window.clearInterval(id);
  }, [adopted]);

  const start = async (op: VolumeOp, volume: string) => {
    setConfirm(null);
    setError(null);
    setJob({ op, volume, percent: null, since: Date.now() });
    try {
      await backend.volumesRun(op, volume);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      const key: Partial<Record<string, I18nKey>> = {
        busy: "maintenance.busy",
        not_allowed: "maint2.error.not_allowed",
        unknown_volume: "maint2.error.unknown_volume",
      };
      const k = key[msg];
      setError({ op, volume, text: k ? t(k) : errorText(e) });
    } finally {
      setJob(null);
      await refresh();
    }
  };

  const blocked = !inTauri || !elevated || repairRunning || job !== null;

  const detail = (o: VolumeOutcome): string | null => {
    if (o.prompted) return t("maint2.detail.prompted");
    if (o.verdict === "failed") {
      if (o.code === 1460) return t("maintenance.code.timeout");
      if (o.code === 40001) return t("maint2.code.denied");
      if (o.code === 43001) return t("maint2.code.fs");
      return t("maintenance.code.other", { code: codeText(o.code) });
    }
    if (o.op === "analyze" && o.fragmentation !== null)
      return t(o.fragmentation >= 10 ? "maint2.detail.frag_high" : "maint2.detail.frag_low", {
        pct: o.fragmentation,
      });
    if (o.verdict === "needs_spot_fix") return t("maint2.detail.needs_spot_fix");
    if (o.verdict === "needs_offline") return t("maint2.detail.needs_offline");
    if (o.verdict === "retry") return t("maint2.detail.retry");
    return null;
  };

  const card = (family: Family, icon: LucideIcon, i: number) => {
    const Icon = icon;
    return (
      <Card key={family} className="panel-hover animate-rise flex flex-col gap-3" style={rise(i)}>
        <div className="flex items-start gap-3">
          <span className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-foreground/5 text-muted-foreground ring-1 ring-foreground/10 ring-inset">
            <Icon className="size-5" />
          </span>
          <div className="min-w-0 flex-1">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="font-display text-[14px] font-semibold tracking-wide">
                {t(`maint2.${family}.title` as I18nKey)}
              </h3>
              <Badge tone={elevated ? "neutral" : "warning"}>{t("common.admin_required")}</Badge>
            </div>
            <p className="mt-1 text-xs leading-relaxed text-muted-foreground">
              {t(`maint2.${family}.body` as I18nKey)}
            </p>
          </div>
        </div>

        {volumes === null ? (
          <div className="flex flex-col gap-2">
            <Skeleton className="h-16 w-full" />
            <Skeleton className="h-16 w-full" />
          </div>
        ) : volumes.length === 0 ? (
          <p className="text-xs text-muted-foreground">{t("maint2.none")}</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {volumes.map((v) => (
              <VolumeRow
                key={v.id}
                v={v}
                family={family}
                last={
                  [...history]
                    .reverse()
                    .find((o) => o.volume === v.id && FAMILY[family].includes(o.op)) ?? null
                }
                job={job?.volume === v.id && FAMILY[family].includes(job.op) ? job : null}
                pending={
                  confirm?.volume === v.id && FAMILY[family].includes(confirm.op)
                    ? confirm.op
                    : null
                }
                blocked={blocked}
                error={
                  error?.volume === v.id && FAMILY[family].includes(error.op) ? error.text : null
                }
                detail={detail}
                locale={locale}
                onAsk={(op) => setConfirm({ op, volume: v.id })}
                onCancel={() => setConfirm(null)}
                onGo={(op) => void start(op, v.id)}
              />
            ))}
          </ul>
        )}
      </Card>
    );
  };

  return (
    <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
      {card("check", HardDrive, index)}
      {card("optimize", Layers, index + 1)}
    </div>
  );
}

function VolumeRow({
  v,
  family,
  last,
  job,
  pending,
  blocked,
  error,
  detail,
  locale,
  onAsk,
  onCancel,
  onGo,
}: {
  v: Volume;
  family: Family;
  last: VolumeOutcome | null;
  job: Job | null;
  pending: VolumeOp | null;
  blocked: boolean;
  error: string | null;
  detail: (o: VolumeOutcome) => string | null;
  locale: string;
  onAsk: (op: VolumeOp) => void;
  onCancel: () => void;
  onGo: (op: VolumeOp) => void;
}) {
  const { t } = useI18n();
  const ops = v.ops.filter((op) => FAMILY[family].includes(op));
  const used = v.size > 0 ? Math.min(100, Math.round(((v.size - v.free) / v.size) * 100)) : 0;
  const note =
    family === "check"
      ? v.needs
        ? t(`maint2.needs.${v.needs}` as I18nKey)
        : ops.length === 0
          ? t("maint2.check.unsupported")
          : null
      : ops.length === 0
        ? t("maint2.optimize.unsupported")
        : t(`maint2.media.note.${v.media}` as I18nKey);
  const extra = last ? detail(last) : null;

  return (
    <li
      className={cn(
        "rounded-lg border border-border bg-background/40 p-3 transition-colors",
        job && "border-primary/40",
      )}
    >
      <div className="flex flex-wrap items-center gap-3">
        <span className="num w-9 font-display text-[22px] leading-none font-semibold">
          {v.letter}:
        </span>
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-center gap-1.5 text-[13px] font-medium">
            <span className="truncate">{v.label || t("maint2.no_label")}</span>
            <Badge tone="neutral">{v.fs || "?"}</Badge>
            <Badge tone="neutral">{t(`maint2.media.${v.media}` as I18nKey)}</Badge>
            {v.system ? <Badge tone="accent">{t("maint2.system")}</Badge> : null}
            {family === "check" ? (
              <Badge tone={HEALTH_TONE[v.health]}>
                {t(`maint2.health.${v.health}` as I18nKey)}
              </Badge>
            ) : null}
          </p>
          <span className="mt-1.5 block h-1 w-full max-w-56 overflow-hidden rounded-full bg-border">
            <span
              className={cn(
                "block h-full rounded-full",
                used >= 90 ? "bg-warning" : "bg-foreground/45",
              )}
              style={{ width: `${used}%` }}
            />
          </span>
          <p className="num mt-1 text-[11px] text-muted-foreground">
            {t("maint2.space", {
              free: formatBytes(v.free, locale),
              size: formatBytes(v.size, locale),
            })}
            {family === "optimize" && v.fragmentation !== null
              ? ` · ${t("maint2.frag", { pct: v.fragmentation })}`
              : ""}
            {family === "optimize" && v.last_optimized
              ? ` · ${t("maint2.last_optimized", {
                  date: new Date(`${v.last_optimized}T12:00:00`).toLocaleDateString(locale),
                })}`
              : ""}
          </p>
        </div>
        {!job && !pending ? (
          <div className="flex flex-wrap gap-2">
            {ops.map((op) => (
              <Button
                key={op}
                size="sm"
                variant={op === "spot_fix" || op === "defrag" ? "outline" : "primary"}
                disabled={blocked}
                onClick={() => onAsk(op)}
              >
                {t(`maint2.op.${op}` as I18nKey)}
              </Button>
            ))}
          </div>
        ) : null}
      </div>

      {note ? (
        <p
          className={cn(
            "mt-2 text-[11px] leading-relaxed",
            family === "check" && v.needs ? "text-warning" : "text-muted-foreground",
          )}
        >
          {note}
        </p>
      ) : null}

      <div className="mt-2 flex items-center gap-2 border-t border-border pt-2 text-xs">
        <span className="text-muted-foreground">{t("maintenance.last")}</span>
        {last ? (
          <>
            <span className="text-muted-foreground/80">{t(`maint2.op.${last.op}` as I18nKey)}</span>
            <Badge tone={TONE[last.verdict]}>
              {t(`maint2.verdict.${last.verdict}` as I18nKey)}
            </Badge>
            <span className="num ml-auto text-muted-foreground">
              {new Date(last.at * 1000).toLocaleString(locale)}
            </span>
          </>
        ) : (
          <span className="text-muted-foreground/70">{t("maintenance.never")}</span>
        )}
      </div>
      {extra ? <p className="mt-1 text-xs text-foreground/85">{extra}</p> : null}

      {job ? (
        <div className="mt-2 flex items-center gap-3">
          <span
            className="relative block h-1.5 flex-1 overflow-hidden rounded-full bg-border"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={job.percent ?? undefined}
          >
            {job.percent !== null ? (
              <span
                className="block h-full rounded-full bg-primary transition-[width] duration-300"
                style={{ width: `${job.percent}%` }}
              />
            ) : (
              <span className="absolute inset-y-0 w-1/3 animate-sweep rounded-full bg-primary/70" />
            )}
          </span>
          <span className="num w-20 text-right text-xs text-muted-foreground">
            {job.percent !== null ? `${job.percent}% · ` : ""}
            {elapsed(Date.now() - job.since)}
          </span>
          <Button variant="ghost" size="sm" onClick={() => void backend.repairCancel()}>
            <Square className="size-3.5" />
            {t("common.stop")}
          </Button>
        </div>
      ) : pending ? (
        <div className="mt-2 flex flex-wrap items-center gap-2 rounded-md bg-warning/8 p-3 ring-1 ring-warning/25 ring-inset">
          <p className="flex-1 text-xs leading-relaxed text-foreground/90">
            {t(
              `maint2.confirm.${pending === "spot_fix" ? (v.system ? "spot_fix_system" : "spot_fix_data") : pending}` as I18nKey,
              { drive: `${v.letter}:` },
            )}
            <span className="mt-1 flex items-center gap-1.5 text-muted-foreground">
              <Clock className="size-3" />
              {t(`maint2.time.${pending}` as I18nKey)}
            </span>
          </p>
          <div className="ml-auto flex gap-2">
            <Button variant="ghost" size="sm" onClick={onCancel}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" size="sm" onClick={() => onGo(pending)}>
              {t("maintenance.confirm.go")}
            </Button>
          </div>
        </div>
      ) : null}
      {error ? <p className="mt-2 text-xs text-destructive">{error}</p> : null}
    </li>
  );
}

/** The labels of Windows' recovery menu, one chip per level. */
function MenuTrail({ steps }: { steps: string[] }) {
  return (
    <p className="mt-2 flex flex-wrap items-center gap-1 rounded-md border border-border bg-background/70 px-2.5 py-2 font-mono text-[11px] leading-none text-foreground/85">
      {steps.map((s, i) => (
        <Fragment key={s}>
          {i > 0 ? <ChevronRight aria-hidden className="size-3 text-muted-foreground" /> : null}
          <span className={i === steps.length - 1 ? "text-primary" : undefined}>{s}</span>
        </Fragment>
      ))}
    </p>
  );
}

/** "Reiniciar nas opções avançadas": where Safe Mode, Startup Repair and System Restore live. */
export function BootOptionsCard({ elevated, index }: { elevated: boolean; index: number }) {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);

  const reboot = async () => {
    setBusy(true);
    setMsg(null);
    try {
      await backend.bootOptionsReboot();
      setMsg(t("maint2.boot.started"));
    } catch (e) {
      setMsg(errorText(e));
    } finally {
      setBusy(false);
      setConfirm(false);
    }
  };

  return (
    <Card className="animate-rise" style={rise(index)}>
      <CardTitle
        help={t("maint2.boot.help")}
        right={
          elevated ? (
            <Button
              variant="outline"
              size="sm"
              disabled={busy || !inTauri}
              onClick={() => setConfirm(true)}
            >
              <Power className="size-4" />
              {t("maint2.boot.button")}
            </Button>
          ) : (
            <ElevateButton route="maintenance" />
          )
        }
      >
        <LifeBuoy className="size-4" />
        {t("maint2.boot.title")}
        <Badge tone={elevated ? "neutral" : "warning"} className="ml-1">
          {t("common.admin_required")}
        </Badge>
      </CardTitle>
      <p className="text-xs leading-relaxed text-muted-foreground">{t("maint2.boot.body")}</p>
      <div className="mt-3 grid grid-cols-1 gap-3 lg:grid-cols-2">
        <div className="rounded-lg border border-border bg-background/40 p-3">
          <p className="text-sm font-semibold">{t("maint2.boot.safe.title")}</p>
          <MenuTrail
            steps={[
              t("maint2.boot.step.troubleshoot"),
              t("maint2.boot.step.advanced"),
              t("maint2.boot.step.startup"),
              t("maint2.boot.step.restart"),
              t("maint2.boot.step.key"),
            ]}
          />
          <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            {t("maint2.boot.safe.body")}
          </p>
        </div>
        <div className="rounded-lg border border-border bg-background/40 p-3">
          <p className="text-sm font-semibold">{t("maint2.boot.other.title")}</p>
          <MenuTrail steps={[t("maint2.boot.step.troubleshoot"), t("maint2.boot.step.advanced")]} />
          <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            {t("maint2.boot.other.body")}
          </p>
        </div>
      </div>
      <p className="mt-3 flex gap-1.5 text-[11px] leading-relaxed text-warning">
        <TriangleAlert className="mt-px size-3.5 shrink-0" />
        {t("maint2.boot.bitlocker")}
      </p>
      {msg ? <p className="animate-rise mt-2 text-xs text-foreground/90">{msg}</p> : null}

      <Modal
        open={confirm}
        onClose={() => !busy && setConfirm(false)}
        title={t("maint2.boot.confirm.title")}
        closeLabel={t("common.cancel")}
        footer={
          <>
            <Button variant="ghost" disabled={busy} onClick={() => setConfirm(false)}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" disabled={busy} onClick={reboot}>
              <Power className="size-4" />
              {t("maint2.boot.confirm.go")}
            </Button>
          </>
        }
      >
        <p>{t("maint2.boot.confirm.body")}</p>
        <p className="mt-2 text-muted-foreground">{t("maint2.boot.confirm.back")}</p>
      </Modal>
    </Card>
  );
}
