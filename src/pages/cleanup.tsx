import { Eye, History, ShieldAlert, Sparkles, Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { SystemFilesPanel } from "@/components/cleanup/system-files";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { LuliCard } from "@/components/luli-card";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  HelpHint,
  Switch,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import {
  CLEANUP_CATALOG,
  type CleanupGroup,
  cleanupHint,
  cleanupName,
  type Risk,
} from "@/data/catalogs";
import {
  backend,
  type CleanupProgress,
  inTauri,
  type PreviewItem,
  type RunRecord,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { sizeText, usePageData } from "@/lib/luli";
import { cn, formatBytes } from "@/lib/utils";

const RISK_KEY: Record<Risk, I18nKey> = {
  low: "cleanup.risk.low",
  medium: "cleanup.risk.medium",
  high: "cleanup.risk.high",
};
const RISK_TONE = { low: "success", medium: "warning", high: "danger" } as const satisfies Record<
  Risk,
  string
>;

type Phase = "idle" | "previewing" | "confirm" | "running";

export function CleanupPage() {
  const { t, locale } = useI18n();
  const errorText = useErrorText();
  const [on, setOn] = useState<Record<string, boolean>>(() =>
    Object.fromEntries(CLEANUP_CATALOG.map((c) => [c.id, c.defaultOn])),
  );
  // "system" = Windows' own cleanup handlers; opened directly from the disk's big-items list.
  const [group, setGroup] = useState<CleanupGroup | "system">(() => {
    if (boot.startTab !== "system") return "daily";
    boot.startTab = undefined;
    return "system";
  });
  const [preview, setPreview] = useState<Record<string, PreviewItem>>({});
  const [phase, setPhase] = useState<Phase>("idle");
  const [progress, setProgress] = useState<CleanupProgress | null>(null);
  const [result, setResult] = useState<RunRecord | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<RunRecord[]>([]);
  const [elevated, setElevated] = useState(false);

  useEffect(() => {
    backend.appInfo().then((i) => i && setElevated(i.elevated));
    backend.cleanupHistory().then((h) => h && setHistory(h));
  }, []);
  useEffect(() => {
    let off: (() => void) | null = null;
    backend.on<CleanupProgress>("cleanup:progress", setProgress).then((u) => {
      off = u;
    });
    return () => off?.();
  }, []);

  const visible = useMemo(() => CLEANUP_CATALOG.filter((c) => c.group === group), [group]);
  const selected = useMemo(() => CLEANUP_CATALOG.filter((c) => on[c.id]).map((c) => c.id), [on]);
  const previewed = selected.every((id) => preview[id]);
  const totalBytes = selected.reduce((s, id) => s + (preview[id]?.bytes ?? 0), 0);
  const needsAdmin = selected.some((id) => preview[id]?.needs_admin);

  const runPreview = useCallback(async () => {
    setPhase("previewing");
    setError(null);
    setResult(null);
    try {
      const items = await backend.cleanupPreview(selected);
      setPreview(Object.fromEntries(items.map((i) => [i.id, i])));
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPhase("idle");
    }
  }, [selected, errorText]);
  const started = useRef(false);
  useEffect(() => {
    if (boot.autorun && inTauri && !started.current) {
      started.current = true;
      void runPreview();
    }
  }, [runPreview]);

  const execute = async () => {
    setPhase("running");
    setProgress({ done: 0, total: selected.length });
    setError(null);
    try {
      const tok = await backend.cleanupConfirm(selected);
      const rec = await backend.cleanupExecute(tok.token, selected);
      setResult(rec);
      setPreview({});
      const h = await backend.cleanupHistory();
      if (h) setHistory(h);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setPhase("idle");
      setProgress(null);
    }
  };

  const busy = phase === "previewing" || phase === "running";

  const luliData = useMemo(() => {
    const rows = Object.values(preview)
      .filter((p) => p.measurable && !p.missing && p.bytes > 0)
      .sort((a, b) => b.bytes - a.bytes);
    if (rows.length === 0) return null;
    return [
      "Cleanup preview measured now (category id: size):",
      ...rows.map(
        (p) =>
          `- ${p.id}: ${sizeText(p.bytes)}, ${p.files} files${p.needs_admin ? ", needs admin" : ""}${p.blocked_by.length ? `, in use by ${p.blocked_by.join(", ")}` : ""}`,
      ),
    ].join("\n");
  }, [preview]);
  usePageData("cleanup", luliData);

  return (
    <>
      <PageHeader
        title={t("cleanup.title")}
        subtitle={t("cleanup.subtitle")}
        tabs={
          <Tabs value={group} onValueChange={(v) => setGroup(v as CleanupGroup | "system")}>
            <TabsList>
              <TabsTrigger value="daily">{t("cleanup.tab.daily")}</TabsTrigger>
              <TabsTrigger value="monthly">{t("cleanup.tab.monthly")}</TabsTrigger>
              <TabsTrigger value="advanced">{t("cleanup.tab.advanced")}</TabsTrigger>
              <TabsTrigger value="system">{t("volcache.tab")}</TabsTrigger>
            </TabsList>
            <TabsContent value="daily" />
            <TabsContent value="monthly" />
            <TabsContent value="advanced" />
            <TabsContent value="system" />
          </Tabs>
        }
      />
      <PageBody>
        <LuliCard
          findings={["system_disk_full"]}
          asks={[
            { label: "luli.ask.cleanup.worth", data: ["cleanup"] },
            { label: "luli.ask.cleanup.often" },
          ]}
        />
        {group === "system" ? (
          <SystemFilesPanel elevated={elevated} onHistory={setHistory} />
        ) : null}
        <Card
          className={cn(
            "animate-rise flex flex-wrap items-center gap-4 py-4",
            group === "system" && "hidden",
          )}
        >
          <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
            {t("cleanup.selected_total", {
              count: selected.length,
              size: previewed && selected.length > 0 ? formatBytes(totalBytes, locale) : "—",
            })}
            <HelpHint text={t("cleanup.help")} />
          </span>
          {needsAdmin && !elevated ? (
            <span className="flex items-center gap-2 text-xs text-warning">
              <ElevateButton route="cleanup" />
              <HelpHint text={t("cleanup.elevate_hint")} />
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
              disabled={busy || !previewed || selected.length === 0}
              onClick={() => setPhase("confirm")}
            >
              <Sparkles className="size-4" />
              {t("cleanup.run")}
            </Button>
          </div>
          {progress ? (
            <p className="w-full text-xs text-muted-foreground">
              {t("cleanup.running", { done: progress.done, total: progress.total })}
            </p>
          ) : null}
          {error ? <p className="w-full text-xs text-destructive">{error}</p> : null}
        </Card>

        {phase === "confirm" && group !== "system" ? (
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
                const c = CLEANUP_CATALOG.find((x) => x.id === id);
                return (
                  <li key={id} className="flex items-center gap-2">
                    <span className={cn(c?.risk === "high" && "text-destructive")}>
                      {t(cleanupName(id))}
                    </span>
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

        {result && group !== "system" ? (
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
                  <span>{t(cleanupName(i.id))}</span>
                  <span className="num ml-auto text-muted-foreground">
                    {i.error
                      ? t("cleanup.result.error", { error: errorText(new Error(i.error)) })
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
            return (
              <Card
                key={c.id}
                className="panel-hover animate-rise flex items-start gap-4"
                style={rise(i)}
              >
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <h3 className="font-display text-[14px] font-semibold tracking-wide">
                      {t(cleanupName(c.id))}
                    </h3>
                    <Badge tone={RISK_TONE[c.risk]}>{t(RISK_KEY[c.risk])}</Badge>
                    {c.admin ? (
                      <Badge
                        tone={p?.needs_admin ? "warning" : "neutral"}
                        title={t("common.admin_required")}
                      >
                        <ShieldAlert className="mr-1 inline size-3" />
                        {t("common.admin_required")}
                      </Badge>
                    ) : null}
                  </div>
                  <p className="mt-1.5 text-xs leading-relaxed text-muted-foreground">
                    {t(cleanupHint(c.id))}
                  </p>
                  {c.requiresClosed ? (
                    <p className="mt-1.5 text-[11px] text-warning/90">
                      {t("cleanup.requires_closed", { apps: c.requiresClosed.join(", ") })}
                    </p>
                  ) : null}
                  {p ? (
                    <p className="num mt-2 flex flex-wrap items-center gap-2 text-xs">
                      {p.missing ? (
                        <span className="text-muted-foreground">
                          {t("cleanup.preview.missing")}
                        </span>
                      ) : p.measurable ? (
                        <>
                          <span className="font-semibold">{formatBytes(p.bytes, locale)}</span>
                          <span className="text-muted-foreground">
                            {t("cleanup.preview.files", { files: p.files })}
                          </span>
                        </>
                      ) : (
                        <span className="text-muted-foreground">
                          {t("cleanup.preview.not_measurable")}
                        </span>
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
                  disabled={busy}
                  onCheckedChange={(v) => setOn((s) => ({ ...s, [c.id]: v }))}
                  aria-label={t(cleanupName(c.id))}
                  className={cn(
                    c.risk === "high" &&
                      "data-[state=checked]:border-destructive data-[state=checked]:bg-destructive/90",
                  )}
                />
              </Card>
            );
          })}
        </div>

        <Card className="animate-rise" style={rise(visible.length)}>
          <CardTitle>
            <History className="size-4" />
            {t("cleanup.history")}
          </CardTitle>
          {history.length === 0 ? (
            <EmptyState
              icon={<History className="size-5" />}
              title={t("cleanup.history.empty")}
              body={t("cleanup.help")}
            />
          ) : (
            <ul className="divide-y divide-border text-xs">
              {[...history]
                .reverse()
                .slice(0, 10)
                .map((r) => (
                  <li key={r.at} className="flex items-center gap-3 py-2">
                    <span className="num text-muted-foreground">
                      {new Date(r.at * 1000).toLocaleString(locale)}
                    </span>
                    <Badge tone="neutral">
                      {t(`cleanup.trigger.${triggerKey(r.trigger)}` as const)}
                    </Badge>
                    <span className="num ml-auto font-semibold">
                      {formatBytes(
                        r.items.reduce((s, i) => s + i.bytes_freed, 0),
                        locale,
                      )}
                    </span>
                    <span className="num text-muted-foreground">
                      {t("cleanup.history.secs", { secs: (r.duration_ms / 1000).toFixed(1) })}
                    </span>
                  </li>
                ))}
            </ul>
          )}
        </Card>
      </PageBody>
    </>
  );
}

function triggerKey(trigger: string): "manual" | "routine" | "game" | "test" {
  return trigger === "routine" || trigger === "game" || trigger === "test" ? trigger : "manual";
}
