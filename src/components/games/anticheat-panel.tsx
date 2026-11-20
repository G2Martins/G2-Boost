import {
  Check,
  CircleHelp,
  RefreshCw,
  ShieldCheck,
  ShieldOff,
  TriangleAlert,
  X,
} from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { BIOS_WARN, BiosRebootButton, MenuPath } from "@/components/bios-guide";
import { rise } from "@/components/shell/page-header";
import { Badge, Button, Card, CardTitle, Modal, Skeleton } from "@/components/ui/primitives";
import {
  type AnticheatCheck,
  type AnticheatReadiness,
  type AnticheatState,
  type AnticheatTitle,
  type AnticheatWindowsFix,
  backend,
  inTauri,
} from "@/lib/backend";
import { type I18nKey, useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const CHIP: Record<AnticheatState, string> = {
  ok: "border-success/30 bg-success/10 text-success",
  missing: "border-destructive/35 bg-destructive/10 text-destructive",
  unknown: "border-border bg-card-hover text-muted-foreground",
};
const ICON = { ok: Check, missing: X, unknown: CircleHelp } as const;

/** One requirement and its state on this PC: the icon carries the state, not only the color. */
function CheckChip({ id, state }: { id: AnticheatCheck; state: AnticheatState }) {
  const { t } = useI18n();
  const Icon = ICON[state];
  return (
    <li
      className={cn(
        "inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[11px] font-semibold",
        CHIP[state],
      )}
      title={t(`anticheat.state.${state}` as I18nKey)}
    >
      <Icon aria-hidden className="size-3" strokeWidth={3} />
      {t(`anticheat.check.${id}` as I18nKey)}
      <span className="sr-only">: {t(`anticheat.state.${state}` as I18nKey)}</span>
    </li>
  );
}

function Chips({ ids, of }: { ids: AnticheatCheck[]; of: (c: AnticheatCheck) => AnticheatState }) {
  return (
    <ul className="flex flex-wrap gap-1.5">
      {ids.map((c) => (
        <CheckChip key={c} id={c} state={of(c)} />
      ))}
    </ul>
  );
}

function ReadyBadge({ title }: { title: AnticheatTitle }) {
  const { t } = useI18n();
  if (title.ready === true) return <Badge tone="success">{t("anticheat.ready")}</Badge>;
  if (title.ready === false)
    return <Badge tone="danger">{t("anticheat.missing", { count: title.missing.length })}</Badge>;
  return <Badge tone="neutral">{t("anticheat.unknown")}</Badge>;
}

function TitleRow({
  title,
  of,
  dense,
}: {
  title: AnticheatTitle;
  of: (c: AnticheatCheck) => AnticheatState;
  dense: boolean;
}) {
  const { t } = useI18n();
  const name = title.installed ?? t(`anticheat.title.${title.id}` as I18nKey);
  const lit = title.ready === true;
  return (
    <li className={cn("flex gap-3", dense ? "py-2.5" : "py-3.5")}>
      <span
        aria-hidden
        className={cn(
          "mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg",
          lit
            ? "bg-success/12 text-success"
            : title.ready === false
              ? "bg-destructive/12 text-destructive"
              : "bg-foreground/6 text-muted-foreground",
        )}
      >
        {lit ? <ShieldCheck className="size-4" /> : <ShieldOff className="size-4" />}
      </span>
      <div className="min-w-0 flex-1">
        <p className="flex flex-wrap items-center gap-2">
          <span className="truncate font-display text-sm font-semibold tracking-wide" title={name}>
            {name}
          </span>
          <span className="text-[11px] text-muted-foreground">
            {t(`anticheat.ac.${title.anticheat}` as I18nKey)}
          </span>
          <span className="ml-auto">
            <ReadyBadge title={title} />
          </span>
        </p>
        <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1.5">
          <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
            {t("anticheat.required")}
          </span>
          <Chips ids={title.required} of={of} />
        </div>
        {!dense && title.recommended.length > 0 ? (
          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1.5">
            <span className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              {t("anticheat.recommended")}
            </span>
            <Chips ids={title.recommended} of={of} />
          </div>
        ) : null}
        {!dense && title.on_demand.length > 0 ? (
          <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1.5">
            <span
              className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground"
              title={t("anticheat.on_demand.hint")}
            >
              {t("anticheat.on_demand")}:{" "}
              <span className={title.on_demand_ok ? "text-success" : undefined}>
                {title.on_demand_ok ? t("anticheat.on_demand.ok") : t("anticheat.on_demand.no")}
              </span>
            </span>
            <Chips ids={title.on_demand} of={of} />
          </div>
        ) : null}
      </div>
    </li>
  );
}

/** Missing Windows steps worth doing for `checks`, MBR2GPT first (it must come before CSM off). */
function windowsSteps(r: AnticheatReadiness, wanted: Set<AnticheatCheck>): AnticheatWindowsFix[] {
  const out: AnticheatWindowsFix[] = [];
  for (const c of r.checks) {
    const fix = c.windows;
    if (c.state !== "missing" || !fix || out.includes(fix)) continue;
    if (wanted.has(c.id) || c.id === "blocklist") out.push(fix);
  }
  return out.sort((a, b) => Number(b === "mbr2gpt") - Number(a === "mbr2gpt"));
}

/** "Pronto para jogar com anti-cheat": per installed game (or every title when none is here). */
export function AnticheatPanel({
  elevated,
  index,
  compact = false,
}: {
  elevated: boolean;
  index: number;
  /** Meu PC: the checks at a glance; the steps live in Jogos and in the BIOS guide below. */
  compact?: boolean;
}) {
  const { t } = useI18n();
  const [r, setR] = useState<AnticheatReadiness | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState<string | null>(null);
  const load = useCallback(async () => {
    setBusy(true);
    const next = await backend.anticheatReadiness();
    if (next) setR(next);
    setBusy(false);
  }, []);
  useEffect(() => {
    void load();
  }, [load]);

  const heading = (
    <>
      <ShieldCheck className="size-4" />
      {compact ? t("anticheat.compact.title") : t("anticheat.title")}
    </>
  );
  const span = compact ? "xl:col-span-2" : undefined;

  if (!inTauri)
    return (
      <Card className={cn("animate-rise", span)} style={rise(index)}>
        <CardTitle help={t("anticheat.help")}>{heading}</CardTitle>
        <p className="text-xs text-muted-foreground">{t("anticheat.app_only")}</p>
      </Card>
    );

  const of = (c: AnticheatCheck): AnticheatState =>
    r?.checks.find((x) => x.id === c)?.state ?? "unknown";
  const installed = r?.titles.filter((x) => x.installed) ?? [];
  const shown = installed.length > 0 ? installed : (r?.titles ?? []);
  const wanted = new Set(shown.flatMap((x) => [...x.required, ...x.recommended, ...x.on_demand]));
  const win = r ? windowsSteps(r, wanted) : [];
  const steps = (r?.guide.items.length ?? 0) + win.length;

  return (
    <Card className={cn("animate-rise", span)} style={rise(index)}>
      <CardTitle
        help={t("anticheat.help")}
        right={
          <div className="flex items-center gap-2">
            <Button
              variant="ghost"
              size="sm"
              disabled={busy}
              onClick={() => void load()}
              aria-label={t("anticheat.recheck")}
            >
              <RefreshCw className={cn("size-4", busy && "animate-spin")} />
              {compact ? null : t("anticheat.recheck")}
            </Button>
            {r && !compact && r.guide.items.length > 0 ? (
              <BiosRebootButton
                uefi={r.guide.uefi}
                elevated={elevated}
                route="games"
                onMessage={setMsg}
              />
            ) : null}
          </div>
        }
      >
        {heading}
        {r && installed.length > 0 ? <Badge tone="neutral">{installed.length}</Badge> : null}
      </CardTitle>

      {r === null ? (
        <div className="flex flex-col gap-2">
          <Skeleton className="h-3.5 w-64" />
          <Skeleton className="h-16 w-full" />
        </div>
      ) : compact ? (
        <>
          <ul className="grid grid-cols-1 gap-x-6 sm:grid-cols-2 xl:grid-cols-5">
            {r.checks.map((c) => (
              <li key={c.id} className="flex items-center justify-between gap-2 py-1.5 text-xs">
                <span className="text-muted-foreground">
                  {t(`anticheat.check.${c.id}` as I18nKey)}
                </span>
                <span
                  className={cn(
                    "font-semibold",
                    c.state === "ok"
                      ? "text-success"
                      : c.state === "missing"
                        ? "text-destructive"
                        : "text-muted-foreground",
                  )}
                >
                  {t(`anticheat.state.${c.state}` as I18nKey)}
                </span>
              </li>
            ))}
          </ul>
          <div className="mt-3 flex flex-wrap items-center gap-2 text-xs">
            {installed.map((x) => (
              <span key={x.id} className="inline-flex items-center gap-1.5">
                <span className="font-semibold">{x.installed}</span>
                <ReadyBadge title={x} />
              </span>
            ))}
            <span className="text-muted-foreground">{t("anticheat.compact.hint")}</span>
          </div>
        </>
      ) : (
        <>
          <p className="text-xs text-muted-foreground">
            {installed.length > 0
              ? t("anticheat.lead.installed", { count: installed.length })
              : t("anticheat.lead.none")}
          </p>
          <ul className="mt-1 divide-y divide-border">
            {shown.map((x) => (
              <TitleRow key={x.id} title={x} of={of} dense={installed.length === 0} />
            ))}
          </ul>

          <div className="mt-3 rounded-lg border border-border bg-background/40 p-3">
            <p className="text-[10px] font-semibold uppercase tracking-wider text-muted-foreground">
              {t("anticheat.steps.title")}
            </p>
            {steps === 0 ? (
              <p className="mt-2 flex items-center gap-2 text-sm text-success">
                <Check className="size-4" strokeWidth={3} />
                {t("anticheat.steps.none")}
              </p>
            ) : (
              <ol className="mt-2 flex flex-col gap-3">
                {win
                  .filter((w) => w === "mbr2gpt")
                  .map((w) => (
                    <WindowsStep key={w} fix={w} />
                  ))}
                {r.guide.items.map((it) => (
                  <li key={it.id} className="text-sm">
                    <p className="flex flex-wrap items-center gap-2 font-semibold">
                      <Badge tone="accent">{t("anticheat.steps.bios")}</Badge>
                      {t(`bios.item.${it.id}` as I18nKey)}
                      {it.optional ? <Badge tone="neutral">{t("bios.optional")}</Badge> : null}
                    </p>
                    <MenuPath path={it.path} />
                    {BIOS_WARN.includes(it.id) ? (
                      <p className="mt-1.5 flex gap-1.5 text-[11px] leading-relaxed text-warning">
                        <TriangleAlert className="mt-px size-3.5 shrink-0" />
                        {t(`bios.item.${it.id}.warn` as I18nKey)}
                      </p>
                    ) : null}
                  </li>
                ))}
                {win
                  .filter((w) => w !== "mbr2gpt")
                  .map((w) => (
                    <WindowsStep key={w} fix={w} />
                  ))}
              </ol>
            )}
            {r.guide.items.length > 0 ? (
              <p className="mt-3 text-[11px] text-muted-foreground">
                {t("bios.keys", { key: r.guide.enter_key })} · {t("bios.names_vary")}
              </p>
            ) : null}
          </div>
          <p className="mt-2 text-[11px] leading-relaxed text-muted-foreground">
            {t("anticheat.note")}
          </p>
          {msg ? <p className="animate-rise mt-2 text-xs text-foreground/90">{msg}</p> : null}
        </>
      )}
    </Card>
  );
}

function WindowsStep({ fix }: { fix: AnticheatWindowsFix }) {
  const { t } = useI18n();
  return (
    <li className="text-sm">
      <p className="flex flex-wrap items-start gap-2">
        <Badge tone="neutral">{t("anticheat.steps.windows")}</Badge>
        <span className="min-w-0 flex-1 text-xs leading-relaxed text-foreground/90">
          {t(`anticheat.fix.${fix}` as I18nKey)}
        </span>
      </p>
    </li>
  );
}

/** Owner's decision: turning memory integrity off with such a game installed only warns, naming it. */
export function HvciOffWarning({
  games,
  open,
  onCancel,
  onConfirm,
}: {
  games: string[];
  open: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  const { t } = useI18n();
  return (
    <Modal
      open={open}
      onClose={onCancel}
      title={t("anticheat.hvci.title")}
      closeLabel={t("anticheat.hvci.keep")}
      footer={
        <>
          <Button variant="primary" onClick={onCancel}>
            {t("anticheat.hvci.keep")}
          </Button>
          <Button variant="ghost" onClick={onConfirm}>
            <ShieldOff className="size-4" />
            {t("anticheat.hvci.confirm")}
          </Button>
        </>
      }
    >
      <p className="flex gap-2 text-warning">
        <TriangleAlert className="mt-0.5 size-4 shrink-0" />
        <span>{t("anticheat.hvci.body", { games: games.join(", ") })}</span>
      </p>
      <p className="mt-2 text-muted-foreground">{t("anticheat.hvci.trade")}</p>
    </Modal>
  );
}
