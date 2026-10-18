import { BatteryCharging, Copy, Pencil, Plug, RefreshCw, Rocket, Trash2, Zap } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useErrorText } from "@/components/elevate-button";
import { rise } from "@/components/shell/page-header";
import {
  Badge,
  Button,
  Card,
  CardTitle,
  HelpHint,
  Modal,
  Skeleton,
  selectClass,
} from "@/components/ui/primitives";
import {
  backend,
  inTauri,
  type PowerPlan,
  type PowerPlans,
  type PowerSetting,
  type PowerSettingId,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Display order and grouping of the core's allowlist (unknown ids are not rendered). */
const GROUPS: { title: I18nKey; ids: PowerSettingId[] }[] = [
  {
    title: "power.group.sleep",
    ids: ["display_off", "sleep_after", "hibernate_after", "disk_off", "wake_timers"],
  },
  { title: "power.group.hardware", ids: ["usb_suspend", "pcie_aspm"] },
  { title: "power.group.cpu", ids: ["cpu_min", "cpu_max", "cpu_boost", "core_parking_min"] },
];

const SECONDS = [0, 60, 120, 180, 300, 600, 900, 1200, 1800, 2700, 3600, 7200, 10800, 18000];
const PERCENT = Array.from({ length: 21 }, (_, i) => i * 5);

type Ask = { kind: "duplicate" | "rename"; plan: PowerPlan; name: string } | null;

export function PowerTab() {
  const { t } = useI18n();
  const errorText = useErrorText();
  const [data, setData] = useState<PowerPlans | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [settings, setSettings] = useState<PowerSetting[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const [ask, setAsk] = useState<Ask>(null);
  const [del, setDel] = useState<PowerPlan | null>(null);

  const load = useCallback(async () => {
    const d = await backend.powerPlans();
    if (!d) return;
    setData(d);
    setSelected((cur) =>
      cur && d.plans.some((p) => p.guid === cur)
        ? cur
        : (d.plans.find((p) => p.active)?.guid ?? null),
    );
  }, []);
  useEffect(() => {
    void load();
  }, [load]);
  useEffect(() => {
    if (!selected) return;
    setSettings(null);
    backend.powerSettings(selected).then((s) => s && setSettings(s));
  }, [selected]);

  const plan = data?.plans.find((p) => p.guid === selected) ?? null;
  const act = async (fn: () => Promise<unknown>, ok: string) => {
    setBusy(true);
    setMsg(null);
    try {
      await fn();
      setMsg(ok);
      await load();
    } catch (e) {
      setMsg(powerError(e));
    } finally {
      setBusy(false);
    }
  };
  const powerError = (e: unknown) => {
    const m = e instanceof Error ? e.message : String(e);
    if (m === "protected_plan") return t("power.error.protected");
    if (m === "invalid_name") return t("power.error.name");
    return errorText(e);
  };

  const write = async (id: PowerSettingId, side: "ac" | "dc", value: number) => {
    if (!plan) return;
    setMsg(null);
    try {
      const v = await backend.powerWrite(
        plan.guid,
        id,
        side === "ac" ? value : null,
        side === "dc" ? value : null,
      );
      setSettings((cur) => cur?.map((s) => (s.id === id ? v : s)) ?? cur);
      setMsg(plan.active ? t("power.saved_active") : t("power.saved_inactive"));
    } catch (e) {
      setMsg(powerError(e));
    }
  };

  const submitAsk = async () => {
    if (!ask) return;
    const { kind, plan: p, name } = ask;
    setAsk(null);
    await act(
      () =>
        kind === "duplicate"
          ? backend.powerDuplicate(p.guid, name)
          : backend.powerRename(p.guid, name),
      kind === "duplicate" ? t("power.duplicated", { name }) : t("power.renamed", { name }),
    );
  };

  const fmtSeconds = (v: number) =>
    v === 0
      ? t("power.never")
      : v < 3600
        ? t("power.minutes", { n: Math.round(v / 60) })
        : t("power.hours", { n: +(v / 3600).toFixed(1) });
  const label = (s: PowerSetting, v: number) =>
    s.kind.type === "seconds"
      ? fmtSeconds(v)
      : s.kind.type === "percent"
        ? `${v}%`
        : t(`power.choice.${s.id}.${v}` as I18nKey);
  const options = (s: PowerSetting, current: number | null) => {
    const base =
      s.kind.type === "seconds" ? SECONDS : s.kind.type === "percent" ? PERCENT : s.kind.values;
    return current === null || base.includes(current)
      ? base
      : [...base, current].sort((a, b) => a - b);
  };
  const byId = new Map(settings?.map((s) => [s.id, s]));

  return (
    <>
      <Card className="animate-rise flex flex-wrap items-center gap-3 py-4">
        <span className="flex items-center gap-2 font-display text-[15px] font-semibold tracking-wide">
          {t("power.title")}
          <HelpHint text={t("power.help")} />
        </span>
        <div className="ml-auto flex gap-2">
          <Button
            variant="outline"
            disabled={busy || !inTauri}
            onClick={load}
            aria-label={t("power.refresh")}
          >
            <RefreshCw className={cn("size-4", busy && "animate-spin")} />
          </Button>
          <Button
            variant="primary"
            disabled={busy || !inTauri}
            onClick={() => act(() => backend.powerCreateMax(), t("power.created_max"))}
          >
            <Rocket className="size-4" />
            {t("power.create_max")}
          </Button>
        </div>
        {msg ? <p className="w-full text-xs text-foreground/90">{msg}</p> : null}
      </Card>

      <div className="grid grid-cols-1 gap-4 xl:grid-cols-[360px_1fr]">
        <Card className="animate-rise py-3" style={rise(1)}>
          <CardTitle className="mb-2">{t("power.plans")}</CardTitle>
          <ul className="space-y-1.5">
            {data === null && inTauri
              ? [0, 1, 2].map((k) => (
                  <li key={k}>
                    <Skeleton className="h-14 w-full" />
                  </li>
                ))
              : null}
            {(data?.plans ?? []).map((p) => (
              <li key={p.guid}>
                <button
                  type="button"
                  onClick={() => setSelected(p.guid)}
                  aria-pressed={p.guid === selected}
                  className={cn(
                    "press w-full rounded-md border px-3 py-2.5 text-left transition-colors duration-200 ease-out-expo",
                    p.guid === selected
                      ? "border-border-strong bg-foreground/[0.06]"
                      : "border-transparent hover:bg-foreground/[0.04]",
                  )}
                >
                  <span className="flex items-center gap-2">
                    <Zap
                      className={cn("size-4", p.active ? "text-primary" : "text-muted-foreground")}
                    />
                    <span className="truncate text-sm font-semibold">{p.name || p.guid}</span>
                    {p.active ? <Badge tone="accent">{t("power.active")}</Badge> : null}
                    {p.builtin ? <Badge tone="neutral">{t("power.builtin")}</Badge> : null}
                  </span>
                </button>
                {p.guid === selected ? (
                  <div className="flex flex-wrap gap-1.5 px-3 pt-2 pb-1">
                    {!p.active ? (
                      <Button
                        size="sm"
                        variant="secondary"
                        disabled={busy}
                        onClick={() =>
                          act(
                            () => backend.powerActivate(p.guid),
                            t("power.activated", { name: p.name }),
                          )
                        }
                      >
                        {t("power.activate")}
                      </Button>
                    ) : null}
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={busy}
                      onClick={() =>
                        setAsk({
                          kind: "duplicate",
                          plan: p,
                          name: t("power.copy_of", { name: p.name }),
                        })
                      }
                    >
                      <Copy className="size-3.5" />
                      {t("power.duplicate")}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={busy}
                      onClick={() => setAsk({ kind: "rename", plan: p, name: p.name })}
                    >
                      <Pencil className="size-3.5" />
                      {t("power.rename")}
                    </Button>
                    <Button
                      size="sm"
                      variant="ghost"
                      disabled={busy || p.builtin || p.active}
                      title={p.builtin || p.active ? t("power.error.protected") : undefined}
                      onClick={() => setDel(p)}
                    >
                      <Trash2 className="size-3.5" />
                      {t("power.delete")}
                    </Button>
                  </div>
                ) : null}
              </li>
            ))}
          </ul>
        </Card>

        <Card className="animate-rise min-w-0" style={rise(2)}>
          <CardTitle help={t("power.settings.help")}>
            {plan ? t("power.settings", { name: plan.name }) : t("power.settings.none")}
          </CardTitle>
          {plan && !plan.active ? (
            <p className="-mt-2 mb-3 text-xs text-muted-foreground">{t("power.inactive_note")}</p>
          ) : null}
          <div className="mb-1 grid grid-cols-[1fr_150px] gap-3 text-[11px] font-semibold text-muted-foreground sm:grid-cols-[1fr_150px_150px]">
            <span />
            <span className="flex items-center gap-1.5">
              <Plug className="size-3.5" />
              {t("power.ac")}
            </span>
            {data?.has_battery ? (
              <span className="flex items-center gap-1.5">
                <BatteryCharging className="size-3.5" />
                {t("power.dc")}
              </span>
            ) : null}
          </div>
          {settings === null && plan
            ? [0, 1, 2, 3, 4].map((k) => <Skeleton key={k} className="my-3 h-8 w-full" />)
            : null}
          {settings
            ? GROUPS.map((g) => {
                const rows = g.ids.map((id) => byId.get(id)).filter((s): s is PowerSetting => !!s);
                if (rows.length === 0) return null;
                return (
                  <section key={g.title} className="mt-3">
                    <p className="mb-1 text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
                      {t(g.title)}
                    </p>
                    <div className="divide-y divide-border">
                      {rows.map((s) => (
                        <div
                          key={s.id}
                          className="grid grid-cols-[1fr_150px] items-center gap-3 py-2 sm:grid-cols-[1fr_150px_150px]"
                        >
                          <span className="flex items-center gap-1.5 text-sm">
                            {t(`power.setting.${s.id}` as I18nKey)}
                            <HelpHint text={t(`power.setting.${s.id}.help` as I18nKey)} />
                          </span>
                          {(data?.has_battery ? (["ac", "dc"] as const) : (["ac"] as const)).map(
                            (side) => {
                              const v = s[side];
                              return v === null ? (
                                <span key={side} className="text-xs text-muted-foreground">
                                  —
                                </span>
                              ) : (
                                <select
                                  key={side}
                                  value={v}
                                  disabled={busy}
                                  aria-label={`${t(`power.setting.${s.id}` as I18nKey)} · ${t(side === "ac" ? "power.ac" : "power.dc")}`}
                                  onChange={(e) => write(s.id, side, Number(e.target.value))}
                                  className={cn(selectClass, "w-full")}
                                >
                                  {options(s, v).map((o) => (
                                    <option key={o} value={o}>
                                      {label(s, o)}
                                    </option>
                                  ))}
                                </select>
                              );
                            },
                          )}
                        </div>
                      ))}
                    </div>
                  </section>
                );
              })
            : null}
        </Card>
      </div>

      <Modal
        open={ask !== null}
        onClose={() => setAsk(null)}
        closeLabel={t("common.cancel")}
        title={ask?.kind === "duplicate" ? t("power.duplicate") : t("power.rename")}
        footer={
          <>
            <Button variant="ghost" onClick={() => setAsk(null)}>
              {t("common.cancel")}
            </Button>
            <Button variant="primary" disabled={!ask?.name.trim()} onClick={submitAsk}>
              {t("power.save")}
            </Button>
          </>
        }
      >
        <label className="flex flex-col gap-1.5 text-xs text-muted-foreground">
          {t("power.name")}
          <input
            value={ask?.name ?? ""}
            maxLength={64}
            onChange={(e) => setAsk((a) => (a ? { ...a, name: e.target.value } : a))}
            onKeyDown={(e) => {
              if (e.key === "Enter" && ask?.name.trim()) void submitAsk();
            }}
            className={cn(selectClass, "w-full text-foreground")}
          />
        </label>
      </Modal>

      <Modal
        open={del !== null}
        onClose={() => setDel(null)}
        closeLabel={t("common.cancel")}
        title={t("power.delete_title", { name: del?.name ?? "" })}
        footer={
          <>
            <Button variant="ghost" onClick={() => setDel(null)}>
              {t("common.cancel")}
            </Button>
            <Button
              variant="danger"
              onClick={() => {
                const p = del;
                setDel(null);
                if (p)
                  void act(() => backend.powerDelete(p.guid), t("power.deleted", { name: p.name }));
              }}
            >
              {t("power.delete")}
            </Button>
          </>
        }
      >
        <p className="text-muted-foreground">{t("power.delete_body")}</p>
      </Modal>
    </>
  );
}
