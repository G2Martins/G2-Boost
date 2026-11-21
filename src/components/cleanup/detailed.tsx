import { BrushCleaning, Eye, ShieldAlert, Trash2, TriangleAlert } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { LivePanel } from "@/components/cleanup/live-panel";
import { useAppExes, useItemName } from "@/components/cleanup/smart-view";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import { ExeIcon } from "@/components/ui/exe-icon";
import { Badge, Button, Card, CardTitle, HelpHint, Switch } from "@/components/ui/primitives";
import { CLEANUP_CATALOG, type CleanupGroup, type Risk } from "@/data/catalogs";
import {
  backend,
  type CleanupProgress,
  inTauri,
  type RunRecord,
  type SmartRun,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { sizeText, usePageData } from "@/lib/luli";
import { cn, formatBytes } from "@/lib/utils";

export type DetailTab = CleanupGroup | "system";
export const DETAIL_TABS: DetailTab[] = ["daily", "monthly", "advanced", "system"];

/** One switch of the detailed mode: a catalog category or a Windows cleanup handler. */
type Item = {
  id: string;
  tab: DetailTab;
  risk: Risk;
  admin: boolean;
  defaultOn: boolean;
  requiresClosed?: string[];
  /** Removes a way back (rollback, older drivers): warned again in the confirmation. */
  warning?: boolean;
};

/** Mirrors `volcache::HANDLERS` (the core validates ids; this list only drives the screen). */
const SYSTEM_FILES: Item[] = [
  ["sys_update_cleanup", "medium", false, true],
  ["sys_previous_installations", "high", false, true],
  ["sys_driver_packages", "medium", false, true],
  ["sys_upgrade_logs", "low", true, false],
  ["sys_temp_setup", "low", true, false],
  ["sys_upgrade_discarded", "low", true, false],
  ["sys_reset_logs", "low", true, false],
  ["sys_setup_logs", "low", true, false],
  ["sys_chkdsk", "low", true, false],
  ["sys_defender", "low", true, false],
  ["sys_memory_dump", "medium", false, false],
].map(([id, risk, defaultOn, warning]) => ({
  id: id as string,
  tab: "system" as const,
  risk: risk as Risk,
  admin: true,
  defaultOn: defaultOn as boolean,
  warning: warning as boolean,
}));

const ITEMS: Item[] = [
  ...CLEANUP_CATALOG.map((c) => ({ ...c, tab: c.group, admin: !!c.admin })),
  ...SYSTEM_FILES,
];

/** What a preview said about one item, catalog or handler alike. */
type Row = {
  bytes: number;
  files: number;
  measurable: boolean;
  missing: boolean;
  needs_admin: boolean;
  blocked_by: string[];
  error: string | null;
};

const RISK_KEY: Record<Risk, I18nKey> = {
  low: "cleanup.risk.low",
  medium: "cleanup.risk.medium",
  high: "cleanup.risk.high",
};
const RISK_TONE = { low: "success", medium: "warning", high: "danger" } as const;

const hint = (id: string) =>
  (id.startsWith("sys_") ? `volcache.h.${id}.hint` : `cleanup.cat.${id}.hint`) as I18nKey;
const warn = (id: string) => `volcache.h.${id}.warn` as I18nKey;

type Phase = "idle" | "previewing" | "confirm" | "running";

/**
 * Detailed mode: every catalog category and Windows handler as a switch, by tab, with one
 * selection, one preview and one run for all of them (catalog and handlers share the queue).
 */
export function DetailedMode({
  tab,
  elevated,
  autorun,
  onHistory,
}: {
  tab: DetailTab;
  elevated: boolean;
  autorun: boolean;
  onHistory: (h: RunRecord[]) => void;
}) {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const name = useItemName();
  const exes = useAppExes();
  const [on, setOn] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(ITEMS.map((c) => [c.id, c.defaultOn])),
  );
  const [preview, setPreview] = useState<Record<string, Row>>({});
  const [phase, setPhase] = useState<Phase>("idle");
  const [live, setLive] = useState<CleanupProgress | null>(null);
  const [startedAt, setStartedAt] = useState(0);
  const [result, setResult] = useState<SmartRun | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const offs = ["smart:measure", "smart:progress"].map((ev) =>
      backend.on<CleanupProgress>(ev, setLive),
    );
    return () => {
      for (const p of offs) p.then((off) => off());
    };
  }, []);

  const visible = useMemo(() => ITEMS.filter((c) => c.tab === tab), [tab]);
  const selected = useMemo(() => ITEMS.filter((c) => on[c.id]).map((c) => c.id), [on]);
  const previewed = selected.length > 0 && selected.every((id) => preview[id]);
  const totalBytes = selected.reduce((s, id) => s + (preview[id]?.bytes ?? 0), 0);
  const needsAdmin = selected.some((id) => preview[id]?.needs_admin);
  const warned = selected.filter((id) => ITEMS.find((c) => c.id === id)?.warning);
  const busy = phase === "previewing" || phase === "running";

  const fail = (e: unknown) => {
    const msg = e instanceof Error ? e.message : String(e);
    setError(msg === "busy" ? t("maintenance.busy") : errorText(e));
  };

  const runPreview = async () => {
    setPhase("previewing");
    setError(null);
    setResult(null);
    setLive({ done: 0, total: selected.length, current: null, bytes: 0, files: 0 });
    setStartedAt(Date.now());
    try {
      const p = await backend.smartPreview(selected);
      const rows: Record<string, Row> = {};
      for (const i of p.catalog) rows[i.id] = { ...i, error: null };
      for (const h of p.system) {
        rows[h.id] = {
          bytes: h.bytes,
          files: 0,
          measurable: true,
          missing: !h.needs_admin && !h.error && (!h.available || h.bytes === 0),
          needs_admin: h.needs_admin,
          blocked_by: [],
          error: h.error,
        };
      }
      setPreview(rows);
    } catch (e) {
      fail(e);
    } finally {
      setPhase("idle");
      setLive(null);
    }
  };
  const started = useRef(false);
  // biome-ignore lint/correctness/useExhaustiveDependencies: one preview on arrival (dev aid)
  useEffect(() => {
    if (autorun && inTauri && !started.current) {
      started.current = true;
      void runPreview();
    }
  }, [autorun]);

  const execute = async () => {
    setPhase("running");
    setLive({ done: 0, total: selected.length, current: null, bytes: 0, files: 0 });
    setStartedAt(Date.now());
    setError(null);
    try {
      // Not elevated: the Windows handlers would only come back "needs admin"; they stay out.
      const ids = elevated ? selected : selected.filter((id) => !id.startsWith("sys_"));
      if (ids.length === 0) {
        setError(t("volcache.needs_admin"));
        return;
      }
      const tok = await backend.smartConfirm(ids);
      setResult(await backend.smartExecute(tok.token, ids));
      setPreview({});
      const h = await backend.cleanupHistory();
      if (h) onHistory(h);
    } catch (e) {
      fail(e);
    } finally {
      setPhase("idle");
      setLive(null);
    }
  };

  const luliData = useMemo(() => {
    const rows = Object.entries(preview)
      .filter(([, p]) => p.measurable && !p.missing && p.bytes > 0)
      .sort((a, b) => b[1].bytes - a[1].bytes);
    if (rows.length === 0) return null;
    return [
      "Cleanup preview measured now (category id: size):",
      ...rows.map(
        ([id, p]) =>
          `- ${id}: ${sizeText(p.bytes)}, ${p.files} files${p.needs_admin ? ", needs admin" : ""}${p.blocked_by.length ? `, in use by ${p.blocked_by.join(", ")}` : ""}`,
      ),
    ].join("\n");
  }, [preview]);
  usePageData("cleanup", luliData);

  const rowText = (id: string, p: Row) => {
    if (p.error) return errorLabel(p.error);
    if (p.missing) return t(id.startsWith("sys_") ? "volcache.nothing" : "cleanup.preview.missing");
    if (!p.measurable) return t("cleanup.preview.not_measurable");
    return null;
  };
  const errorLabel = (code: string) =>
    code === "missing"
      ? t("volcache.error.missing")
      : code === "untrusted_handler"
        ? t("volcache.error.untrusted")
        : code === "cancelled"
          ? t("volcache.error.cancelled")
          : code === "busy"
            ? t("maintenance.busy")
            : t("volcache.error.generic", { code });

  const items = result?.records.flatMap((r) => r.items) ?? [];

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-4 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("cleanup.selected_total", {
            count: selected.length,
            size: previewed ? formatBytes(totalBytes, locale) : "—",
          })}
          <HelpHint text={tab === "system" ? t("volcache.help") : t("cleanup.help")} />
        </span>
        {(needsAdmin || tab === "system") && !elevated ? (
          <span className="flex items-center gap-2 text-xs text-warning">
            <ElevateButton route="cleanup" tab={tab} />
            <HelpHint
              text={tab === "system" ? t("volcache.needs_admin") : t("cleanup.elevate_hint")}
            />
          </span>
        ) : null}
        <div className="ml-auto flex gap-2">
          <Button
            variant="outline"
            disabled={busy || !inTauri || selected.length === 0}
            onClick={runPreview}
          >
            <Eye className="size-4" />
            {phase === "previewing" ? t("cleanup.preview.running") : t("cleanup.preview")}
          </Button>
          <Button
            variant="primary"
            disabled={busy || !previewed}
            onClick={() => setPhase("confirm")}
          >
            <BrushCleaning className="size-4" />
            {t("cleanup.run")}
          </Button>
        </div>
        {error ? <p className="w-full text-xs text-destructive">{error}</p> : null}
      </Card>

      {live && busy ? (
        <LivePanel
          live={live}
          running={phase === "running"}
          ids={selected}
          since={startedAt}
          expected={phase === "running" ? totalBytes : 0}
          nameOf={name}
          onStop={() => void backend.smartCancel()}
          opaque={(id) => id.startsWith("sys_") || preview[id]?.measurable === false}
          noteOf={(id) => (id === "dism" ? "cleanup.live.component" : undefined)}
        />
      ) : null}

      {phase === "confirm" ? (
        <Card className="animate-rise border-warning/40">
          <CardTitle>
            <Trash2 className="size-4 text-warning" />
            {t("cleanup.confirm.title")}
          </CardTitle>
          <p className="text-sm text-foreground/90">
            {t("cleanup.confirm.body", {
              size: formatBytes(totalBytes, locale),
              count: selected.length,
            })}
          </p>
          <ul className="mt-3 grid grid-cols-1 gap-1 text-xs md:grid-cols-2">
            {selected.map((id) => {
              const p = preview[id];
              const c = ITEMS.find((x) => x.id === id);
              return (
                <li key={id} className="flex items-center gap-2">
                  <span className={cn(c?.risk === "high" && "text-destructive")}>{name(id)}</span>
                  <span className="num ml-auto text-muted-foreground">
                    {p?.measurable
                      ? formatBytes(p.bytes, locale)
                      : t("cleanup.preview.not_measurable")}
                  </span>
                  {p?.needs_admin && !elevated ? (
                    <Badge tone="warning">{t("cleanup.needs_admin")}</Badge>
                  ) : null}
                </li>
              );
            })}
          </ul>
          {warned.length > 0 ? (
            <ul className="mt-3 flex flex-col gap-1.5 rounded-md bg-destructive/8 p-3 text-xs ring-1 ring-destructive/25 ring-inset">
              {warned.map((id) => (
                <li key={id} className="flex items-start gap-2 text-destructive">
                  <TriangleAlert className="mt-px size-3.5 shrink-0" />
                  <span>
                    <span className="font-semibold">{name(id)}:</span> {t(warn(id))}
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
                  items.reduce((s, i) => s + i.bytes_freed, 0),
                  locale,
                ),
              })}
            </span>
          </CardTitle>
          <ul className="grid grid-cols-1 gap-1 text-xs md:grid-cols-2">
            {items.map((i) => (
              <li key={i.id} className="flex items-center gap-2">
                <span>{name(i.id)}</span>
                <span className="num ml-auto text-muted-foreground">
                  {i.error
                    ? i.error === "needs_admin"
                      ? t("cleanup.needs_admin")
                      : i.id.startsWith("sys_")
                        ? errorLabel(i.error)
                        : t("cleanup.result.error", { error: errorText(new Error(i.error)) })
                    : `${formatBytes(i.bytes_freed, locale)}${
                        i.skipped > 0
                          ? ` · ${t("cleanup.result.skipped", { count: i.skipped })}`
                          : ""
                      }`}
                </span>
              </li>
            ))}
          </ul>
        </Card>
      ) : null}

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        {visible.map((c, i) => {
          const p = preview[c.id];
          const note = p ? rowText(c.id, p) : null;
          return (
            <Card
              key={c.id}
              className="panel-hover animate-rise flex items-start gap-4"
              style={rise(i)}
            >
              <ExeIcon path={exes[c.id]} name={name(c.id)} className="mt-0.5 size-5" />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <h3 className="font-display text-[14px] font-semibold tracking-wide">
                    {name(c.id)}
                  </h3>
                  <Badge tone={RISK_TONE[c.risk]}>{t(RISK_KEY[c.risk])}</Badge>
                  {c.admin ? (
                    <Badge
                      tone={
                        p?.needs_admin || (c.tab === "system" && !elevated) ? "warning" : "neutral"
                      }
                      title={t("common.admin_required")}
                    >
                      <ShieldAlert className="mr-1 inline size-3" />
                      {t("common.admin_required")}
                    </Badge>
                  ) : null}
                </div>
                <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
                  {t(hint(c.id))}
                </p>
                {c.warning ? (
                  <p className="mt-1.5 flex items-start gap-1.5 text-[11px] text-warning">
                    <TriangleAlert className="mt-px size-3 shrink-0" />
                    {t(warn(c.id))}
                  </p>
                ) : null}
                {c.requiresClosed ? (
                  <p className="mt-1.5 text-[11px] text-warning/90">
                    {t("cleanup.requires_closed", { apps: c.requiresClosed.join(", ") })}
                  </p>
                ) : null}
                {p ? (
                  <p className="num mt-2 flex flex-wrap items-center gap-2 text-xs">
                    {note ? (
                      <span className="text-muted-foreground">{note}</span>
                    ) : p.needs_admin && c.tab === "system" ? (
                      <span className="text-muted-foreground">{t("cleanup.needs_admin")}</span>
                    ) : (
                      <>
                        <span className="font-semibold">{formatBytes(p.bytes, locale)}</span>
                        {p.files > 0 ? (
                          <span className="text-muted-foreground">
                            {t("cleanup.preview.files", { files: p.files })}
                          </span>
                        ) : null}
                      </>
                    )}
                    {p.blocked_by.length > 0 ? (
                      <span className="text-warning">
                        {t("cleanup.preview.blocked", { apps: p.blocked_by.join(", ") })}
                      </span>
                    ) : null}
                  </p>
                ) : null}
              </div>
              <Switch
                checked={!!on[c.id]}
                // Confirming: the list on screen is what runs, nothing joins it without a preview.
                disabled={busy || phase === "confirm"}
                onCheckedChange={(v) => setOn((s) => ({ ...s, [c.id]: v }))}
                aria-label={name(c.id)}
                className={cn(
                  c.risk === "high" &&
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
