import {
  BellRing,
  FolderOpen,
  PackageX,
  RefreshCw,
  Shield,
  ShieldAlert,
  ShieldCheck,
  ShieldQuestionMark,
  Undo2,
} from "lucide-react";
import { type CSSProperties, useCallback, useEffect, useState } from "react";
import { ElevateButton, useErrorText } from "@/components/elevate-button";
import { LuliCard } from "@/components/luli-card";
import { AntivirusTab } from "@/components/security/antivirus-tab";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import { ExeIcon } from "@/components/ui/exe-icon";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  EmptyState,
  Modal,
  Skeleton,
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/primitives";
import {
  backend,
  type ExposureAlert,
  type ExposureCheck,
  type ExposureCheckId,
  type ExposureFix,
  type ExposureItem,
  type ExposureReport,
  type ExposureSeverity,
  inTauri,
} from "@/lib/backend";
import { boot } from "@/lib/boot";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { usePageData } from "@/lib/luli";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";

const GROUPS: { id: "remote" | "network" | "system"; checks: ExposureCheckId[] }[] = [
  {
    id: "remote",
    checks: ["rdp", "nla", "remote_assistance", "winrm", "sessions", "remote_apps"],
  },
  {
    id: "network",
    checks: ["firewall", "upnp", "inbound", "listeners", "smb1_server", "smb1_client"],
  },
  { id: "system", checks: ["antivirus", "persistence", "guest"] },
];
const ORDER = GROUPS.flatMap((g) => g.checks);
const RANK: Record<ExposureSeverity, number> = { info: 0, low: 1, medium: 2, high: 3 };
const SEV_TONE = {
  high: "danger",
  medium: "warning",
  low: "neutral",
  info: "neutral",
} as const;
/** Fill of one "door" in the perimeter strip. */
const DOOR: Record<ExposureSeverity, string> = {
  high: "bg-destructive",
  medium: "bg-warning",
  low: "bg-warning/45",
  info: "bg-foreground/30",
};
const LIST_LIMIT = 6;

/** Removing a router mapping and disabling a user task need no admin; the rest does. */
const needsAdmin = (f: ExposureFix) => f.kind !== "upnp_remove" && f.kind !== "task_disable";
const sameFix = (a: ExposureFix, b: ExposureFix) => JSON.stringify(a) === JSON.stringify(b);
const isIssue = (c: ExposureCheck) => c.state === "exposed" && c.severity !== "info";

export function SecurityPage() {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [tab, setTab] = useState(boot.startTab === "antivirus" ? "antivirus" : "exposure");
  const [report, setReport] = useState<ExposureReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [alerts, setAlerts] = useState<ExposureAlert[]>([]);
  const [monitor, setMonitor] = useState<boolean | null>(null);
  const [pending, setPending] = useState<{ fix: ExposureFix; name?: string } | null>(null);
  const [working, setWorking] = useState(false);
  const [note, setNote] = useState<{ ok: boolean; text: string } | null>(null);

  const load = useCallback(async () => {
    setBusy(true);
    const r = await backend.exposureReport();
    if (r) setReport(r);
    setBusy(false);
  }, []);
  useEffect(() => {
    void load();
    backend.exposureAlerts().then((a) => a && setAlerts(a));
    backend.settingsGet().then((s) => s && setMonitor(s.exposure_monitor));
    const off = backend.on<ExposureAlert[]>("exposure:alerts", setAlerts);
    return () => {
      void off.then((f) => f());
    };
  }, [load]);

  const fail = (e: unknown) => {
    const msg = e instanceof Error ? e.message : String(e);
    const own = `security.error.${msg}`;
    return t(own as I18nKey) !== own ? t(own as I18nKey) : errorText(e);
  };
  const run = async (action: () => Promise<void>, ok: I18nKey) => {
    setWorking(true);
    setNote(null);
    try {
      await action();
      setNote({ ok: true, text: t(ok) });
      await load();
    } catch (e) {
      setNote({ ok: false, text: fail(e) });
    }
    setWorking(false);
  };

  const checks = ORDER.map((id) => report?.checks.find((c) => c.id === id)).filter(
    (c): c is ExposureCheck => !!c,
  );
  usePageData(
    "security.report",
    report
      ? checks
          .map(
            (c) =>
              `- ${c.id}: ${c.state}${c.state === "exposed" ? ` (${c.severity})` : ""}${c.items.length ? `: ${c.items.map((i) => `${i.name}${i.port ? `:${i.port}` : ""}${i.reason ? ` ${i.reason}` : ""}`).join(", ")}` : ""}`,
          )
          .join("\n")
      : null,
  );

  const fixText = (f: ExposureFix, name?: string) => {
    const p: Record<string, string | number> = { name: name ?? "" };
    if (f.kind === "firewall_on") p.profile = t(`security.profile.${f.profile}` as const);
    if (f.kind === "upnp_remove") {
      p.protocol = f.protocol;
      p.port = f.external_port;
    }
    if (f.kind === "task_disable") p.name = f.path;
    return p;
  };
  const elevated = report?.elevated ?? false;
  const ask = (fix: ExposureFix, name?: string) => setPending({ fix, name });

  return (
    <>
      <PageHeader
        title={t("security.title")}
        subtitle={t("security.subtitle")}
        actions={
          tab === "exposure" ? (
            <Button variant="outline" size="sm" disabled={busy || !inTauri} onClick={load}>
              <RefreshCw className={cn("size-4", busy && "animate-spin")} />
              {busy ? t("security.scanning") : t("security.refresh")}
            </Button>
          ) : null
        }
        tabs={
          <Tabs value={tab} onValueChange={setTab}>
            <TabsList>
              <TabsTrigger value="exposure">{t("av.tab.exposure")}</TabsTrigger>
              <TabsTrigger value="antivirus">{t("av.tab.antivirus")}</TabsTrigger>
            </TabsList>
            <TabsContent value="antivirus">
              <PageBody className="px-0 pb-0">
                <AntivirusTab />
              </PageBody>
            </TabsContent>
            <TabsContent value="exposure">
              <PageBody className="px-0 pb-0">
                <p className="animate-rise flex items-center gap-2 text-xs text-muted-foreground">
                  <Shield className="size-3.5 shrink-0" />
                  {t("security.disclaimer")}
                </p>
                {note ? (
                  <p className={cn("text-xs", note.ok ? "text-success" : "text-destructive")}>
                    {note.text}
                  </p>
                ) : null}

                {!inTauri ? (
                  <Card className="animate-rise">
                    <EmptyState
                      icon={<ShieldQuestionMark className="size-5" />}
                      title={t("security.empty")}
                    />
                  </Card>
                ) : (
                  <>
                    <LuliCard
                      findings={[
                        "rdp_open",
                        "nla_off",
                        "firewall_off",
                        "smb1_on",
                        "upnp_open",
                        "remote_inbound",
                        "av_off",
                      ]}
                      asks={[
                        { label: "security.ask.explain", data: ["security.report"] },
                        { label: "security.ask.remote", data: ["security.report"] },
                      ]}
                    />
                    <Summary
                      checks={report ? checks : null}
                      elevated={elevated}
                      tookMs={report?.took_ms ?? null}
                      onFix={ask}
                    />
                    {alerts.length > 0 ? (
                      <AlertsCard
                        alerts={alerts}
                        onAck={() => {
                          void backend.exposureAlertsClear();
                          setAlerts([]);
                        }}
                      />
                    ) : null}
                    {report && report.reverts.length > 0 ? (
                      <Card className="animate-rise">
                        <CardTitle>
                          <Undo2 className="size-4" />
                          {t("security.reverts.title")}
                        </CardTitle>
                        <ul className="divide-y divide-border">
                          {report.reverts.map((f) => (
                            <li
                              key={JSON.stringify(f)}
                              className="flex items-center gap-3 py-2 text-sm"
                            >
                              <span className="min-w-0 flex-1 truncate">
                                {t(`security.done.${f.kind}` as const, fixText(f))}
                              </span>
                              <Button
                                size="sm"
                                variant="ghost"
                                disabled={working || (needsAdmin(f) && !elevated)}
                                onClick={() =>
                                  void run(() => backend.exposureRevert(f), "security.reverted")
                                }
                              >
                                <Undo2 className="size-4" />
                                {t("security.reverts.undo")}
                              </Button>
                            </li>
                          ))}
                        </ul>
                      </Card>
                    ) : null}
                    {GROUPS.map((g, gi) => (
                      <Card key={g.id} className="animate-rise" style={rise(gi + 2)}>
                        <CardTitle help={t(`security.group.${g.id}.hint` as const)}>
                          {t(`security.group.${g.id}` as const)}
                        </CardTitle>
                        <ul className="divide-y divide-border">
                          {g.checks.map((id) => {
                            const c = report?.checks.find((x) => x.id === id);
                            return c ? (
                              <CheckRow
                                key={id}
                                check={c}
                                elevated={elevated}
                                busy={working}
                                onFix={ask}
                                onError={(e) => setNote({ ok: false, text: fail(e) })}
                              />
                            ) : (
                              <li key={id} className="flex items-center gap-3 py-3.5">
                                <Skeleton className="size-8 rounded-md" />
                                <div className="flex-1 space-y-1.5">
                                  <Skeleton className="h-3.5 w-48" />
                                  <Skeleton className="h-3 w-80 max-w-full" />
                                </div>
                              </li>
                            );
                          })}
                        </ul>
                      </Card>
                    ))}
                    <p className="text-xs text-muted-foreground">
                      {monitor === false ? t("security.monitor.off") : t("security.monitor.on")}{" "}
                      <button
                        type="button"
                        onClick={() => navigate("settings")}
                        className="underline underline-offset-2 hover:text-foreground"
                      >
                        {t("security.monitor.settings")}
                      </button>
                    </p>
                  </>
                )}
              </PageBody>
            </TabsContent>
          </Tabs>
        }
      />

      <Modal
        open={pending !== null}
        onClose={() => setPending(null)}
        title={t("security.confirm.title")}
        closeLabel={t("common.cancel")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setPending(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="primary"
              disabled={working}
              onClick={() => {
                const p = pending;
                setPending(null);
                if (p) void run(() => backend.exposureFix(p.fix), "security.applied");
              }}
            >
              {t("security.confirm.apply")}
            </Button>
          </>
        }
      >
        {pending ? (
          <div className="space-y-2">
            <p>
              {t(
                `security.confirm.${pending.fix.kind}` as const,
                fixText(pending.fix, pending.name),
              )}
            </p>
            <p className="text-xs text-muted-foreground">{t("security.confirm.backup")}</p>
          </div>
        ) : null}
      </Modal>
    </>
  );
}

/* ---------------- summary: one "door" per check + what to do now ---------------- */

function Summary({
  checks,
  elevated,
  tookMs,
  onFix,
}: {
  checks: ExposureCheck[] | null;
  elevated: boolean;
  tookMs: number | null;
  onFix: (f: ExposureFix, name?: string) => void;
}) {
  const { t } = useI18n();
  const issues = (checks ?? []).filter(isIssue).sort((a, b) => RANK[b.severity] - RANK[a.severity]);
  const worst = issues[0]?.severity;
  const posture = worst === "high" ? "high" : worst === "medium" ? "medium" : worst ? "low" : "ok";
  const count = (s: ExposureSeverity) => issues.filter((c) => c.severity === s).length;
  const unknown = (checks ?? []).filter((c) => c.state === "unknown").length;
  const adminFix = issues.some((c) => headlineFix(c) && needsAdmin(headlineFix(c) as ExposureFix));
  return (
    <Card
      className="animate-rise grid gap-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]"
      style={rise(1)}
    >
      <div className="min-w-0">
        <p className="text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
          {t("security.posture.title")}
        </p>
        {checks ? (
          <>
            <p
              className={cn(
                "mt-1 font-display text-[28px] font-semibold leading-tight tracking-wide",
                posture === "high" && "text-destructive",
                posture === "medium" && "text-warning",
                posture === "ok" && "text-success",
              )}
            >
              {t(`security.posture.${posture}` as const)}
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              {t(`security.posture.body_${posture}` as const)}
            </p>
          </>
        ) : (
          <div className="mt-2 space-y-2">
            <Skeleton className="h-7 w-40" />
            <Skeleton className="h-3 w-64" />
          </div>
        )}
        {/* The perimeter: one door per check, colored by what it lets in. */}
        <div className="mt-5 flex h-9 items-stretch gap-1" aria-hidden>
          {(checks ?? ORDER.map(() => null)).map((c, i) => (
            <span
              // biome-ignore lint/suspicious/noArrayIndexKey: fixed-length strip, order never changes
              key={i}
              title={c ? t(`security.check.${c.id}.title` as const) : undefined}
              className={cn(
                "flex-1 rounded-[3px] transition-colors duration-300",
                !c && "skeleton",
                c?.state === "ok" && "bg-foreground/8",
                c?.state === "unknown" &&
                  "border border-dashed border-border-strong bg-transparent",
                c?.state === "exposed" && DOOR[c.severity],
              )}
            />
          ))}
        </div>
        <p className="num mt-2 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-muted-foreground">
          {(["high", "medium", "low"] as const).map((s) => (
            <span key={s} className="inline-flex items-center gap-1.5">
              <span className={cn("size-2 rounded-xs", DOOR[s])} />
              {count(s)} {t(`security.sev.${s}` as const)}
            </span>
          ))}
          {unknown ? <span>{t("security.unknown_count", { n: unknown })}</span> : null}
          {tookMs !== null ? (
            <span className="ml-auto">{t("security.took", { s: (tookMs / 1000).toFixed(1) })}</span>
          ) : null}
        </p>
      </div>

      <div className="min-w-0 lg:border-l lg:border-border lg:pl-6">
        <p className="text-[10px] font-semibold uppercase tracking-[0.14em] text-muted-foreground">
          {t("security.todo.title")}
        </p>
        {checks && issues.length === 0 ? (
          <p className="mt-2 text-sm text-muted-foreground">{t("security.todo.none")}</p>
        ) : null}
        <ol className="mt-2 space-y-1.5">
          {issues.slice(0, 4).map((c, i) => {
            const fix = headlineFix(c);
            return (
              <li key={c.id} className="flex items-center gap-3">
                <span className="num w-4 text-xs text-muted-foreground">{i + 1}</span>
                <span className={cn("size-2 shrink-0 rounded-full", DOOR[c.severity])} />
                <span className="min-w-0 flex-1 truncate text-sm">
                  {t(`security.check.${c.id}.title` as const)}
                  <span className="text-muted-foreground">
                    {" · "}
                    {statusText(c, t)}
                  </span>
                </span>
                {fix ? (
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={needsAdmin(fix) && !elevated}
                    title={needsAdmin(fix) && !elevated ? t("security.admin_needed") : undefined}
                    onClick={() => onFix(fix, c.items[0]?.name)}
                  >
                    {t(`security.fix.${fix.kind}` as const)}
                  </Button>
                ) : (
                  <a
                    href={`#sec-${c.id}`}
                    className="text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground"
                    onClick={(e) => {
                      e.preventDefault();
                      document
                        .getElementById(`sec-${c.id}`)
                        ?.scrollIntoView({ behavior: "smooth", block: "center" });
                    }}
                  >
                    {t("security.see")}
                  </a>
                )}
              </li>
            );
          })}
        </ol>
        {adminFix && !elevated ? (
          <div className="mt-4 flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
            <span>{t("security.admin_needed")}</span>
            <ElevateButton route="security" />
          </div>
        ) : null}
      </div>
    </Card>
  );
}

/** The fix a check offers as a whole, or the one of its first item. */
function headlineFix(c: ExposureCheck): ExposureFix | null {
  return c.fix ?? c.items.find((i) => i.fix)?.fix ?? null;
}

function statusText(c: ExposureCheck, t: ReturnType<typeof useI18n>["t"]): string {
  if (c.state === "unknown") {
    const key = `security.unknown.${c.details.reason ?? "unreadable"}`;
    return t(key as I18nKey) !== key ? t(key as I18nKey) : t("security.unknown.unreadable");
  }
  if (c.state === "ok") return t(`security.check.${c.id}.ok` as const);
  if (c.id === "antivirus" && c.items.some((i) => i.reason === "outdated")) {
    return t("security.check.antivirus.outdated");
  }
  return t(`security.check.${c.id}.exposed` as const, {
    n: c.items.length,
    port: c.details.port ?? "",
  });
}

/* ---------------- one check ---------------- */

function CheckRow({
  check: c,
  elevated,
  busy,
  onFix,
  onError,
}: {
  check: ExposureCheck;
  elevated: boolean;
  busy: boolean;
  onFix: (f: ExposureFix, name?: string) => void;
  onError: (e: unknown) => void;
}) {
  const { t } = useI18n();
  const [all, setAll] = useState(false);
  const Icon =
    c.state === "ok" ? ShieldCheck : c.state === "unknown" ? ShieldQuestionMark : ShieldAlert;
  const tone =
    c.state === "ok"
      ? "text-success bg-success/10"
      : c.state === "unknown"
        ? "text-muted-foreground bg-foreground/5"
        : c.severity === "high"
          ? "text-destructive bg-destructive/10"
          : c.severity === "medium"
            ? "text-warning bg-warning/10"
            : "text-muted-foreground bg-foreground/6";
  const items = all ? c.items : c.items.slice(0, LIST_LIMIT);
  const fixButton = (f: ExposureFix, name?: string, style?: CSSProperties) => (
    <Button
      size="sm"
      variant="outline"
      style={style}
      disabled={busy || (needsAdmin(f) && !elevated)}
      title={needsAdmin(f) && !elevated ? t("security.admin_needed") : undefined}
      onClick={() => onFix(f, name)}
    >
      {t(`security.fix.${f.kind}` as const)}
    </Button>
  );
  return (
    <li id={`sec-${c.id}`} className="scroll-mt-6 py-3.5">
      <div className="flex items-start gap-3">
        <span className={cn("flex size-8 shrink-0 items-center justify-center rounded-md", tone)}>
          <Icon className="size-4" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="flex flex-wrap items-center gap-2 text-sm font-medium">
            {t(`security.check.${c.id}.title` as const)}
            <Badge
              tone={
                c.state === "ok"
                  ? "success"
                  : c.state === "unknown"
                    ? "neutral"
                    : SEV_TONE[c.severity]
              }
            >
              {c.state === "exposed" && c.severity !== "info"
                ? t(`security.sev.${c.severity}` as const)
                : t(`security.state.${c.state}` as const)}
            </Badge>
            <span className="text-xs font-normal text-muted-foreground">{statusText(c, t)}</span>
          </p>
          <p className="mt-0.5 max-w-3xl text-xs leading-relaxed text-muted-foreground">
            {t(`security.check.${c.id}.body` as const)}
          </p>
          {c.details.third_party ? (
            <p className="mt-1 text-xs text-muted-foreground">
              {t("security.check.firewall.third_party", { name: c.details.third_party })}
            </p>
          ) : null}
          {c.details.policy ? (
            <p className="mt-1 text-xs text-warning">{t("security.error.managed_by_policy")}</p>
          ) : null}
          {c.details.custom_port ? (
            <p className="mt-1 text-xs text-muted-foreground">
              {t("security.check.rdp.custom_port", { port: c.details.port ?? "" })}
            </p>
          ) : null}
          {c.id === "smb1_server" && c.state === "exposed" ? (
            <p className="mt-1 text-xs text-muted-foreground">{t("security.reboot")}</p>
          ) : null}
          {items.length > 0 ? (
            <ul className="mt-2.5 divide-y divide-border/60 rounded-md border border-border bg-background/40">
              {items.map((it) => (
                <ItemRow
                  key={`${it.name}-${it.port ?? ""}-${it.remote ?? ""}-${it.path ?? ""}`}
                  check={c.id}
                  item={it}
                  fix={
                    it.fix && !(c.fix && sameFix(it.fix, c.fix)) ? fixButton(it.fix, it.name) : null
                  }
                  onError={onError}
                />
              ))}
            </ul>
          ) : null}
          {c.items.length > LIST_LIMIT ? (
            <button
              type="button"
              onClick={() => setAll((v) => !v)}
              className="mt-1.5 text-xs text-muted-foreground underline underline-offset-2 hover:text-foreground"
            >
              {all ? t("security.less") : t("security.more", { n: c.items.length })}
            </button>
          ) : null}
        </div>
        {c.fix ? <div className="shrink-0">{fixButton(c.fix, c.items[0]?.name)}</div> : null}
      </div>
    </li>
  );
}

function ItemRow({
  check,
  item: it,
  fix,
  onError,
}: {
  check: ExposureCheckId;
  item: ExposureItem;
  fix: React.ReactNode;
  onError: (e: unknown) => void;
}) {
  const { t } = useI18n();
  const label = (prefix: string, v: string | null) => {
    if (!v) return null;
    const key = `${prefix}.${v}`;
    const s = t(key as I18nKey);
    return s !== key ? s : v.toUpperCase();
  };
  const name = check === "firewall" ? t(`security.profile.${it.name}` as I18nKey) : it.name;
  const meta = [
    it.port !== null ? `${it.protocol ? `${it.protocol.toUpperCase()} ` : ""}${it.port}` : null,
    label("security.reason", it.reason),
    check === "antivirus" ? null : label("security.source", it.source),
    it.remote
      ? check === "inbound"
        ? t("security.from", { ip: it.remote })
        : check === "upnp"
          ? t("security.to", { target: it.remote })
          : it.remote
      : null,
  ].filter(Boolean);
  const uninstall = check === "remote_apps" || (check === "persistence" && it.source === "service");
  return (
    <li className="flex items-center gap-2.5 px-2.5 py-1.5">
      {it.path ? (
        <ExeIcon path={it.path} name={it.name} className="size-4" />
      ) : (
        <span className={cn("ml-1 size-2 shrink-0 rounded-full", DOOR[it.severity])} />
      )}
      <span className="min-w-0 flex-1">
        <span className="block truncate text-[13px]" title={it.path ?? undefined}>
          {name}
        </span>
        {meta.length ? (
          <span className="block truncate text-[11px] text-muted-foreground">
            {meta.join(" · ")}
          </span>
        ) : null}
      </span>
      {it.severity !== "info" && it.severity !== "low" ? (
        <Badge tone={SEV_TONE[it.severity]}>{t(`security.sev.${it.severity}` as const)}</Badge>
      ) : null}
      {it.path ? (
        <Button
          size="sm"
          variant="ghost"
          aria-label={t("security.reveal")}
          title={t("security.reveal")}
          onClick={() => backend.exposureReveal(it.path as string).catch(onError)}
        >
          <FolderOpen className="size-4" />
        </Button>
      ) : null}
      {uninstall ? (
        <Button
          size="sm"
          variant="ghost"
          aria-label={t("security.uninstall")}
          title={t("security.uninstall")}
          onClick={() => navigate("apps")}
        >
          <PackageX className="size-4" />
        </Button>
      ) : null}
      {fix}
    </li>
  );
}

/* ---------------- monitor alerts ---------------- */

function AlertsCard({ alerts, onAck }: { alerts: ExposureAlert[]; onAck: () => void }) {
  const { t, locale } = useI18n();
  const time = new Intl.DateTimeFormat(locale, { hour: "2-digit", minute: "2-digit" });
  return (
    <Card className="animate-rise ring-1 ring-warning/25">
      <CardTitle
        right={
          <Button size="sm" variant="outline" onClick={onAck}>
            {t("security.alerts.ack")}
          </Button>
        }
      >
        <BellRing className="size-4 text-warning" />
        {t("security.alerts.title")}
      </CardTitle>
      <ul className="space-y-1.5 text-sm">
        {alerts.map((a) => (
          <li key={`${a.kind}-${a.name}-${a.at}`} className="flex gap-3">
            <span className="num w-12 shrink-0 text-xs text-muted-foreground">
              {time.format(new Date(a.at * 1000))}
            </span>
            <span className="min-w-0">
              {t(`security.alert.${a.kind}` as const, { name: a.name, port: a.port ?? "—" })}
            </span>
          </li>
        ))}
      </ul>
    </Card>
  );
}
