import {
  Check,
  CircleAlert,
  Flag,
  Gauge,
  LayoutGrid,
  Loader2,
  type LucideIcon,
  Rocket,
  Search,
  Sparkle,
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useState } from "react";
import { ElevateButton } from "@/components/elevate-button";
import { PageBody, PageHeader, rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  Meter,
  Skeleton,
  Switch,
} from "@/components/ui/primitives";
import { toast } from "@/components/ui/toast";
import { TWEAKS, tweakDesc, tweakName } from "@/data/catalogs";
import {
  backend,
  type InstallEntry,
  type InstallProgress,
  inTauri,
  type TweakState,
} from "@/lib/backend";
import { useI18n } from "@/lib/i18n";
import { navigate } from "@/lib/nav";
import { cn } from "@/lib/utils";

/** Comfort extras offered after a fresh install (each one is a normal reversible tweak). */
const EXTRAS = ["show_extensions", "classic_context_menu", "taskbar_left"];
const EXTRAS_ON = new Set(["show_extensions"]);

type Line = { key: string; label: string; ok: boolean | null; note?: string };

/**
 * Pós-formatação: apps in one batch + debloat + recommended tweaks + comfort extras, on one
 * screen. Everything goes through the normal flows (tweak backups, install token), so the
 * Restauração tab can undo it item by item.
 */
export function SetupPage() {
  const { t } = useI18n();
  const [elevated, setElevated] = useState(false);
  const [catalog, setCatalog] = useState<InstallEntry[] | null>(null);
  const [installed, setInstalled] = useState<Set<string>>(new Set());
  const [winget, setWinget] = useState(true);
  const [states, setStates] = useState<TweakState[] | null>(null);
  const [recs, setRecs] = useState<string[]>([]);
  const [apps, setApps] = useState<Set<string>>(new Set());
  const [picked, setPicked] = useState<Set<string>>(new Set());
  const [query, setQuery] = useState("");
  const [restore, setRestore] = useState(true);
  const [phase, setPhase] = useState<"pick" | "running" | "done">("pick");
  const [lines, setLines] = useState<Line[]>([]);

  useEffect(() => {
    if (!inTauri) return;
    void (async () => {
      const [info, cat, status, tw, rec] = await Promise.all([
        backend.appInfo(),
        backend.installCatalog(),
        backend.installStatus(false),
        backend.boostState(),
        backend.recommendations(),
      ]);
      setElevated(info?.elevated ?? false);
      const have = new Set((status?.states ?? []).filter((s) => s.installed).map((s) => s.id));
      setInstalled(have);
      setWinget(status?.winget ?? true);
      setCatalog(cat ?? []);
      setApps(new Set((cat ?? []).filter((e) => e.essential && !have.has(e.id)).map((e) => e.id)));
      setStates(tw ?? []);
      const pending = rec?.score.pending ?? [];
      setRecs(pending);
      const off = new Set((tw ?? []).filter((s) => s.status === "off").map((s) => s.id));
      const pre = [
        ...TWEAKS.filter((w) => w.level === "essential").map((w) => w.id),
        ...pending,
        ...EXTRAS.filter((id) => EXTRAS_ON.has(id)),
      ].filter((id) => off.has(id));
      setPicked(new Set(pre));
    })();
  }, []);

  const off = useMemo(
    () => new Set((states ?? []).filter((s) => s.status === "off").map((s) => s.id)),
    [states],
  );
  const debloat = TWEAKS.filter((w) => w.level === "essential" || w.level === "recommended").map(
    (w) => w.id,
  );
  const recOnly = recs.filter((id) => !debloat.includes(id) && !EXTRAS.includes(id));
  const chosen = [...picked].filter((id) => off.has(id));
  const needsAdmin = chosen.filter((id) => TWEAKS.find((w) => w.id === id)?.admin && !elevated);
  const shownApps = (catalog ?? []).filter((e) => {
    const q = query.trim().toLowerCase();
    return q ? `${e.name} ${e.id}`.toLowerCase().includes(q) : e.essential || apps.has(e.id);
  });

  const toggle = (set: Set<string>, id: string, on: boolean) => {
    const next = new Set(set);
    if (on) next.add(id);
    else next.delete(id);
    return next;
  };

  const run = async () => {
    setPhase("running");
    const log: Line[] = [];
    const push = (l: Line) => {
      log.push(l);
      setLines([...log]);
    };
    if (restore && elevated) {
      try {
        const out = await backend.restoreCreate(t("setup.restore.desc"));
        push({
          key: "restore",
          label: t("setup.log.restore"),
          ok: true,
          note: out === "created" ? undefined : t("setup.log.restore_recent"),
        });
      } catch {
        push({ key: "restore", label: t("setup.log.restore"), ok: false });
      }
    }
    for (const id of chosen) {
      const label = t(tweakName(id));
      if (TWEAKS.find((w) => w.id === id)?.admin && !elevated) {
        push({ key: id, label, ok: false, note: t("setup.log.needs_admin") });
        continue;
      }
      try {
        await backend.boostApply(id, true);
        push({ key: id, label, ok: true });
      } catch (e) {
        push({ key: id, label, ok: false, note: e instanceof Error ? e.message : String(e) });
      }
    }
    const ids = [...apps].filter((id) => !installed.has(id));
    if (ids.length && winget) {
      const names = new Map((catalog ?? []).map((e) => [e.id, e.name]));
      const off = await backend.on<InstallProgress>("install:progress", (p) => {
        setLines([
          ...log,
          {
            key: `app:${p.id}`,
            label: names.get(p.id) ?? p.id,
            ok: null,
            note: t("setup.log.installing", { index: p.index + 1, total: p.total }),
          },
        ]);
      });
      try {
        const token = await backend.installConfirm(ids);
        const result = await backend.installRun(token.token, ids);
        for (const it of result.items) {
          push({
            key: `app:${it.id}`,
            label: names.get(it.id) ?? it.name,
            ok: it.status === "ok",
            note: it.status === "ok" ? undefined : it.reason,
          });
        }
      } catch (e) {
        push({
          key: "apps",
          label: t("setup.step.apps"),
          ok: false,
          note: e instanceof Error ? e.message : String(e),
        });
      } finally {
        off();
      }
    }
    setLines([...log]);
    setPhase("done");
    const failed = log.filter((l) => l.ok === false).length;
    toast(t("setup.done.toast", { ok: log.length - failed, failed }), {
      tone: failed ? "warning" : "success",
    });
  };

  const total = chosen.length + [...apps].filter((id) => !installed.has(id)).length;

  return (
    <PageHeader
      title={t("nav.setup")}
      subtitle={t("setup.subtitle")}
      tabs={
        <PageBody className="px-0 pb-28">
          {phase === "pick" ? (
            <>
              <Step
                icon={LayoutGrid}
                title={t("setup.step.apps")}
                hint={t("setup.step.apps.hint")}
                index={0}
              >
                {!winget ? (
                  <p className="text-xs text-warning">{t("setup.winget_missing")}</p>
                ) : null}
                <label className="relative mb-3 block max-w-sm">
                  <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted-foreground" />
                  <input
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder={t("setup.apps.search")}
                    className="h-9 w-full rounded-md border border-border bg-background pr-3 pl-9 text-sm outline-none focus:border-ring"
                  />
                </label>
                {catalog === null ? (
                  <SkeletonGrid />
                ) : (
                  <div className="grid grid-cols-2 gap-1.5 md:grid-cols-3 xl:grid-cols-4">
                    {shownApps.map((e) => (
                      <Pick
                        key={e.id}
                        label={e.name}
                        sub={installed.has(e.id) ? t("setup.apps.installed") : undefined}
                        checked={apps.has(e.id)}
                        disabled={installed.has(e.id)}
                        onChange={(on) => setApps((s) => toggle(s, e.id, on))}
                      />
                    ))}
                  </div>
                )}
              </Step>
              <TweakStep
                index={1}
                icon={Flag}
                title={t("setup.step.debloat")}
                hint={t("setup.step.debloat.hint")}
                ids={debloat}
                off={off}
                states={states}
                picked={picked}
                onPick={(id, on) => setPicked((s) => toggle(s, id, on))}
              />
              <TweakStep
                index={2}
                icon={Gauge}
                title={t("setup.step.recs")}
                hint={t("setup.step.recs.hint")}
                ids={recOnly}
                off={off}
                states={states}
                picked={picked}
                onPick={(id, on) => setPicked((s) => toggle(s, id, on))}
              />
              <TweakStep
                index={3}
                icon={Sparkle}
                title={t("setup.step.extras")}
                hint={t("setup.step.extras.hint")}
                ids={EXTRAS}
                off={off}
                states={states}
                picked={picked}
                onPick={(id, on) => setPicked((s) => toggle(s, id, on))}
              />
            </>
          ) : (
            <Card className="animate-rise">
              <CardTitle>
                {phase === "running" ? (
                  <Loader2 className="size-4 animate-spin text-primary" />
                ) : (
                  <Check className="size-4 text-success" />
                )}
                {phase === "running" ? t("setup.running") : t("setup.done.title")}
              </CardTitle>
              <Meter
                value={
                  total
                    ? lines.filter((l) => l.ok !== null).length /
                      (total + (restore && elevated ? 1 : 0))
                    : 1
                }
                className="mb-4"
                label={t("setup.running")}
              />
              <ul className="grid gap-1.5 text-xs">
                {lines.map((l) => (
                  <li key={l.key} className="flex items-center gap-2.5">
                    {l.ok === null ? (
                      <Loader2 className="size-3.5 shrink-0 animate-spin text-muted-foreground" />
                    ) : l.ok ? (
                      <Check className="size-3.5 shrink-0 text-success" />
                    ) : (
                      <CircleAlert className="size-3.5 shrink-0 text-warning" />
                    )}
                    <span className="min-w-0 truncate">{l.label}</span>
                    {l.note ? (
                      <span className="ml-auto shrink-0 text-muted-foreground">{l.note}</span>
                    ) : null}
                  </li>
                ))}
              </ul>
              {phase === "done" ? (
                <div className="mt-5 flex flex-wrap gap-2">
                  <Button variant="primary" onClick={() => navigate("profiles")}>
                    {t("setup.go_restore")}
                  </Button>
                  <Button variant="outline" onClick={() => navigate("dashboard")}>
                    {t("setup.go_dash")}
                  </Button>
                </div>
              ) : null}
            </Card>
          )}
          {phase === "pick" ? (
            <div className="panel fixed right-6 bottom-6 left-auto z-30 flex max-w-[calc(100vw-3rem)] flex-wrap items-center gap-4 bg-surface/95 px-4 py-3 backdrop-blur">
              <span className="flex items-center gap-2 text-[13px] font-semibold">
                <Rocket className="size-4 text-primary" />
                {t("setup.summary", {
                  apps: [...apps].filter((id) => !installed.has(id)).length,
                  tweaks: chosen.length,
                })}
              </span>
              {needsAdmin.length ? (
                <span className="flex items-center gap-2 text-xs text-warning">
                  {t("setup.admin_skip", { count: needsAdmin.length })}
                  <ElevateButton route="setup" />
                </span>
              ) : null}
              {elevated ? (
                <span className="flex items-center gap-2 text-xs">
                  <Switch
                    checked={restore}
                    onCheckedChange={setRestore}
                    aria-label={t("setup.restore")}
                  />
                  {t("setup.restore")}
                </span>
              ) : null}
              <Button variant="primary" tactile disabled={!inTauri || total === 0} onClick={run}>
                {t("setup.go")}
              </Button>
            </div>
          ) : null}
        </PageBody>
      }
    />
  );
}

function Step({
  icon: Icon,
  title,
  hint,
  index,
  children,
}: {
  icon: LucideIcon;
  title: string;
  hint: string;
  index: number;
  children: ReactNode;
}) {
  return (
    <Card className="animate-rise" style={rise(index)}>
      <header className="mb-4 flex items-start gap-3">
        <span className="num flex size-8 shrink-0 items-center justify-center rounded-lg border border-border-strong bg-background text-xs font-semibold text-primary">
          {index + 1}
        </span>
        <div className="min-w-0">
          <h3 className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
            <Icon className="size-4 text-muted-foreground" />
            {title}
          </h3>
          <p className="mt-0.5 text-xs text-muted-foreground">{hint}</p>
        </div>
      </header>
      {children}
    </Card>
  );
}

function TweakStep({
  index,
  icon,
  title,
  hint,
  ids,
  off,
  states,
  picked,
  onPick,
}: {
  index: number;
  icon: LucideIcon;
  title: string;
  hint: string;
  ids: string[];
  off: Set<string>;
  states: TweakState[] | null;
  picked: Set<string>;
  onPick: (id: string, on: boolean) => void;
}) {
  const { t } = useI18n();
  const known = new Set((states ?? []).map((s) => s.id));
  const open = ids.filter((id) => off.has(id));
  const done = ids.filter((id) => known.has(id) && !off.has(id)).length;
  return (
    <Step icon={icon} title={title} hint={hint} index={index}>
      {states === null ? (
        <SkeletonGrid />
      ) : open.length === 0 ? (
        <p className="text-xs text-muted-foreground">{t("setup.none")}</p>
      ) : (
        <div className="grid gap-1.5 md:grid-cols-2">
          {open.map((id) => (
            <Pick
              key={id}
              label={t(tweakName(id))}
              sub={t(tweakDesc(id))}
              badge={TWEAKS.find((w) => w.id === id)?.admin ? t("setup.admin") : undefined}
              checked={picked.has(id)}
              onChange={(on) => onPick(id, on)}
            />
          ))}
        </div>
      )}
      {done ? (
        <p className="mt-3 text-[11px] text-muted-foreground">
          {t("setup.already", { count: done })}
        </p>
      ) : null}
    </Step>
  );
}

function Pick({
  label,
  sub,
  badge,
  checked,
  disabled,
  onChange,
}: {
  label: string;
  sub?: string;
  badge?: string;
  checked: boolean;
  disabled?: boolean;
  onChange: (on: boolean) => void;
}) {
  return (
    <label
      className={cn(
        "press flex cursor-pointer items-start gap-2.5 rounded-md border border-border px-3 py-2 transition-colors hover:border-border-strong",
        checked && "border-primary/40 bg-primary/6",
        disabled && "cursor-default opacity-50",
      )}
    >
      <input
        type="checkbox"
        className="mt-0.5 accent-[hsl(var(--primary))]"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="min-w-0">
        <span className="flex items-center gap-1.5 text-[13px] font-medium">
          <span className="truncate">{label}</span>
          {badge ? <Badge tone="neutral">{badge}</Badge> : null}
        </span>
        {sub ? <span className="line-clamp-2 text-[11px] text-muted-foreground">{sub}</span> : null}
      </span>
    </label>
  );
}

function SkeletonGrid() {
  return (
    <div className="grid grid-cols-2 gap-1.5 md:grid-cols-3">
      {["a", "b", "c", "d", "e", "f"].map((k) => (
        <Skeleton key={k} className="h-11" />
      ))}
    </div>
  );
}
