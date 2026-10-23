import { Eye, ShieldAlert, Sparkles, Square, Trash2, TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, HelpHint, Switch } from "@/components/ui/primitives";
import type { Risk } from "@/data/catalogs";
import {
  backend,
  type CleanupProgress,
  inTauri,
  type RunRecord,
  type VolCacheItem,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn, formatBytes } from "@/lib/utils";

/** Mirrors `volcache::HANDLERS` (the core validates ids; this list only drives the screen). */
const SYSTEM_FILES: { id: string; risk: Risk; defaultOn: boolean; warning: boolean }[] = [
  { id: "sys_update_cleanup", risk: "medium", defaultOn: false, warning: true },
  { id: "sys_previous_installations", risk: "high", defaultOn: false, warning: true },
  { id: "sys_driver_packages", risk: "medium", defaultOn: false, warning: true },
  { id: "sys_upgrade_logs", risk: "low", defaultOn: true, warning: false },
  { id: "sys_temp_setup", risk: "low", defaultOn: true, warning: false },
  { id: "sys_upgrade_discarded", risk: "low", defaultOn: true, warning: false },
  { id: "sys_reset_logs", risk: "low", defaultOn: true, warning: false },
  { id: "sys_setup_logs", risk: "low", defaultOn: true, warning: false },
  { id: "sys_chkdsk", risk: "low", defaultOn: true, warning: false },
  { id: "sys_defender", risk: "low", defaultOn: true, warning: false },
  { id: "sys_memory_dump", risk: "medium", defaultOn: false, warning: false },
];

const RISK_KEY: Record<Risk, I18nKey> = {
  low: "cleanup.risk.low",
  medium: "cleanup.risk.medium",
  high: "cleanup.risk.high",
};
const RISK_TONE = { low: "success", medium: "warning", high: "danger" } as const;

const name = (id: string) => `volcache.h.${id}` as I18nKey;
const hint = (id: string) => `volcache.h.${id}.hint` as I18nKey;
const warn = (id: string) => `volcache.h.${id}.warn` as I18nKey;

type Phase = "idle" | "previewing" | "confirm" | "running";

/** "System files" tab of the cleanup page: Windows' own Disk Cleanup handlers. */
export function SystemFilesPanel({
  elevated,
  onHistory,
}: {
  elevated: boolean;
  onHistory: (h: RunRecord[]) => void;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [on, setOn] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(SYSTEM_FILES.map((h) => [h.id, h.defaultOn])),
  );
  const [preview, setPreview] = useState<Record<string, VolCacheItem>>({});
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<CleanupProgress | null>(null);
  const [result, setResult] = useState<RunRecord | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let off: (() => void) | null = null;
    backend.on<CleanupProgress>("volcache:progress", setProgress).then((u) => {
      off = u;
    });
    return () => off?.();
  }, []);

  const selected = useMemo(() => SYSTEM_FILES.filter((h) => on[h.id]).map((h) => h.id), [on]);
  const previewed = selected.length > 0 && selected.every((id) => preview[id]);
  const totalBytes = selected.reduce((s, id) => s + (preview[id]?.bytes ?? 0), 0);
  const warned = selected.filter((id) => SYSTEM_FILES.find((h) => h.id === id)?.warning);
  const busy = phase === "previewing" || phase === "running";

  const fail = (e: unknown) => {
    const msg = e instanceof Error ? e.message : String(e);
    setError(msg === "busy" ? t("maintenance.busy") : errorText(e));
  };

  const runPreview = async () => {
    setPhase("previewing");
    setError(null);
    setResult(null);
    setProgress({ done: 0, total: selected.length });
    try {
      const items = await backend.volcachePreview(selected);
      setPreview(Object.fromEntries(items.map((i) => [i.id, i])));
    } catch (e) {
      fail(e);
    } finally {
      setPhase("idle");
      setProgress(null);
    }
  };

  const execute = async () => {
    setPhase("running");
    setProgress({ done: 0, total: selected.length });
    setError(null);
    try {
      const tok = await backend.volcacheConfirm(selected);
      setResult(await backend.volcacheExecute(tok.token, selected));
      setPreview({});
      const h = await backend.cleanupHistory();
      if (h) onHistory(h);
    } catch (e) {
      fail(e);
    } finally {
      setPhase("idle");
      setProgress(null);
    }
  };

  const itemText = (p: VolCacheItem) => {
    if (p.needs_admin) return t("cleanup.needs_admin");
    if (p.error) return errorLabel(p.error);
    if (!p.available || p.bytes === 0) return t("volcache.nothing");
    return formatBytes(p.bytes, locale);
  };
  const errorLabel = (code: string) =>
    code === "missing"
      ? t("volcache.error.missing")
      : code === "untrusted_handler"
        ? t("volcache.error.untrusted")
        : code === "cancelled"
          ? t("volcache.error.cancelled")
          : t("volcache.error.generic", { code });

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-4 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("cleanup.selected_total", {
            count: selected.length,
            size: previewed ? formatBytes(totalBytes, locale) : "—",
          })}
          <HelpHint text={t("volcache.help")} />
        </span>
        {!elevated ? (
          <span className="flex items-center gap-2 text-xs text-warning">
            <ElevateButton route="cleanup" tab="system" />
            <HelpHint text={t("volcache.needs_admin")} />
          </span>
        ) : null}
        <div className="ml-auto flex gap-2">
          {busy ? (
            <Button variant="ghost" onClick={() => void backend.repairCancel()}>
              <Square className="size-4" />
              {t("common.stop")}
            </Button>
          ) : null}
          <Button
            variant="outline"
            disabled={busy || !inTauri || !elevated || selected.length === 0}
            onClick={runPreview}
          >
            <Eye className="size-4" />
            {phase === "previewing" ? t("cleanup.preview.running") : t("cleanup.preview")}
          </Button>
          <Button
            variant="primary"
            disabled={busy || !previewed || !elevated}
            onClick={() => setPhase("confirm")}
          >
            <Sparkles className="size-4" />
            {t("cleanup.run")}
          </Button>
        </div>
        {progress ? (
          <p className="w-full text-xs text-muted-foreground">
            {t(phase === "running" ? "cleanup.running" : "volcache.measuring", {
              done: progress.done,
              total: progress.total,
            })}
            <span className="ml-2 text-muted-foreground/70">{t("volcache.slow")}</span>
          </p>
        ) : null}
        {error ? <p className="w-full text-xs text-destructive">{error}</p> : null}
      </Card>

      {phase === "confirm" ? (
        <Card className="animate-rise border-warning/40">
          <CardTitle>
            <Trash2 className="size-4 text-warning" />
            {t("volcache.confirm.title")}
          </CardTitle>
          <p className="text-sm text-foreground/90">
            {t("volcache.confirm.body", {
              size: formatBytes(totalBytes, locale),
              count: selected.length,
            })}
          </p>
          {warned.length > 0 ? (
            <ul className="mt-3 flex flex-col gap-1.5 rounded-md bg-destructive/8 p-3 text-xs ring-1 ring-destructive/25 ring-inset">
              {warned.map((id) => (
                <li key={id} className="flex items-start gap-2 text-destructive">
                  <TriangleAlert className="mt-px size-3.5 shrink-0" />
                  <span>
                    <span className="font-semibold">{t(name(id))}:</span> {t(warn(id))}
                  </span>
                </li>
              ))}
            </ul>
          ) : null}
          <div className="mt-4 flex justify-end gap-2">
            <Button variant="ghost" onClick={() => setPhase("idle")}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" onClick={execute}>
              <Trash2 className="size-4" />
              {t("cleanup.confirm.go")}
            </Button>
          </div>
        </Card>
      ) : null}

      {result ? (
        <Card className="animate-rise">
          <CardTitle>
            {t("cleanup.result.title")}
            <span className="ml-2 font-sans text-xs font-normal text-success">
              {t("cleanup.result.freed", {
                size: formatBytes(
                  result.items.reduce((s, i) => s + i.bytes_freed, 0),
                  locale,
                ),
              })}
            </span>
          </CardTitle>
          <ul className="grid grid-cols-1 gap-1 text-xs md:grid-cols-2">
            {result.items.map((i) => (
              <li key={i.id} className="flex items-center gap-2">
                <span>{t(name(i.id))}</span>
                <span className="num ml-auto text-muted-foreground">
                  {i.error
                    ? i.error === "needs_admin"
                      ? t("cleanup.needs_admin")
                      : errorLabel(i.error)
                    : formatBytes(i.bytes_freed, locale)}
                </span>
              </li>
            ))}
          </ul>
        </Card>
      ) : null}

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        {SYSTEM_FILES.map((h, i) => {
          const p = preview[h.id];
          return (
            <Card
              key={h.id}
              className="panel-hover animate-rise flex items-start gap-4"
              style={rise(i)}
            >
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="font-display text-[14px] font-semibold tracking-wide">
                    {t(name(h.id))}
                  </h3>
                  <Badge tone={RISK_TONE[h.risk]}>{t(RISK_KEY[h.risk])}</Badge>
                  <Badge tone={elevated ? "neutral" : "warning"} title={t("common.admin_required")}>
                    <ShieldAlert className="mr-1 inline size-3" />
                    {t("common.admin_required")}
                  </Badge>
                </div>
                <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
                  {t(hint(h.id))}
                </p>
                {h.warning ? (
                  <p className="mt-1.5 flex items-start gap-1.5 text-[11px] text-warning">
                    <TriangleAlert className="mt-px size-3 shrink-0" />
                    {t(warn(h.id))}
                  </p>
                ) : null}
                {p ? (
                  <p
                    className={cn(
                      "num mt-2 text-xs",
                      p.available && p.bytes > 0 && !p.error
                        ? "font-semibold"
                        : "text-muted-foreground",
                    )}
                  >
                    {itemText(p)}
                  </p>
                ) : null}
              </div>
              <Switch
                checked={!!on[h.id]}
                disabled={busy}
                onCheckedChange={(v) => setOn((s) => ({ ...s, [h.id]: v }))}
                aria-label={t(name(h.id))}
                className={cn(
                  h.risk === "high" &&
                    "data-[state=checked]:border-destructive data-[state=checked]:bg-destructive/90",
                )}
              />
            </Card>
          );
        })}
      </div>
    </>
  );
}
